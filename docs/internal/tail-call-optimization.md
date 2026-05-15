# Tail Call Optimization

現在の Surtr における末尾呼び出し最適化（TCO）の適用範囲メモです。

この文書は「今の実装で何が TCO 対象になるか」を手早く確認するための補助資料です。正本の VM 契約は [../dev/EldrVM_spec.md](../dev/EldrVM_spec.md)、導入時の作業ログは [../../doc/optimize/001_tail_call_optimization.md](../../doc/optimize/001_tail_call_optimization.md) を参照してください。

## 要点

- 対象は user function への末尾 call です
- 判定は bytecode 上で「`Call` / `CallClosure` の直後が `Return` か」、または `CallClosure; Return` から圧縮された `TailCallClosure` かで決まります
- top-level call 自体は再利用対象にしません
- `CallClosure` / `TailCallClosure` は target が user function のときだけ `tail_calls_optimized` の対象です

## TCO 対象になる形

現在の実装では、関数本体または closure 本体の末尾位置にある call が対象です。

### 1. 関数本体の最後がそのまま call

```surtr
def fib_tail(n: Int, a: Int, b: Int) -> Int {
  if(n == 0, a, fib_tail(n - 1, b, a + b))
}
```

`fib_tail(...)` は末尾位置なので対象です。

### 2. block の最後が call

```surtr
def loop(n: Int, acc: Int) -> Int {
  {
    next = acc + 1
    loop(n - 1, next)
  }
}
```

block 内の前半文は通常評価され、最後の `loop(...)` が対象です。

### 3. `if` の各 branch の最後が call

```surtr
def even(n: Int) -> Boolean {
  if(n == 0, True, odd(n - 1))
}

def odd(n: Int) -> Boolean {
  if(n == 0, False, even(n - 1))
}
```

`then` / `else` の返り値位置にある call は対象です。相互再帰も同じです。

### 4. `match` の各 arm の最後が call

```surtr
def sum_list(values: List<Int>, acc: Int) -> Int {
  match values {
    [] => acc,
    [head, ..tail] => sum_list(tail, acc + head),
  }
}
```

各 arm の末尾にある call は対象です。

### 5. callable value 経由でも、target が user function なら対象

```surtr
def loop(next: (Int, Int -> Int), n: Int, acc: Int) -> Int {
  if(n == 0, acc, next(n - 1, acc + 1))
}
```

この形は `CallClosure` になりますが、実体が user function なら対象です。

## TCO 対象にならない形

### 1. call の後に処理が残る

```surtr
def sum_non_tail(n: Int) -> Int {
  if(n == 0, 0, 1 + sum_non_tail(n - 1))
}
```

`sum_non_tail(...)` の返り値に `1 + ...` が続くので対象外です。

### 2. block の最後ではない call

```surtr
def f(n: Int) -> Int {
  {
    g(n)
    0
  }
}
```

`g(n)` のあとに評価が残るので対象外です。

### 3. `;` で値を捨てる call

```surtr
def f(n: Int) -> Unit {
  g(n);
}
```

この形では call 結果を捨てて `Unit` を返すため、対象外です。

### 4. `if` に `else` がなく、branch の値がそのまま返り値にならない

```surtr
def f(n: Int) -> Unit {
  if(n > 0, g(n))
}
```

現在の codegen では `then` 側を通常評価して捨て、最終的に `Unit` を返します。`g(n)` は対象外です。

### 5. top-level call

```surtr
fib_tail(50, 0, 1)
```

top-level 実行開始用の frame は再利用しません。  
そのため tail-recursive な関数でも `max_frame_depth` は通常 1 ではなく 2 になります。

### 6. builtin target への `CallClosure`

callable value の target が builtin の場合は、user-function TCO 対象にしていません。
`TailCallClosure` に圧縮されている場合は現在 frame の caller へ直接返りますが、
`tail_calls_optimized` には含めません。

## 実装上の見方

現状の TCO は通常の `Call` / `CallClosure` と、`CallClosure; Return` を圧縮した `TailCallClosure` を使って実装しています。

- Forge:
  - 関数本体と closure 本体の末尾位置を tail-position 用に codegen する
  - `Block` の最後、`if` の各 branch、`match` の各 arm で `Call` / `CallClosure` の直後に `Return` が並ぶ形を作る
  - ラベル境界を跨がない `CallClosure; Return` を `TailCallClosure` へ畳み込む
- Eldr:
  - 非 top-level frame 上で、次 opcode が `Return` のとき current frame を再利用する
  - user function target の `TailCallClosure` でも current frame を再利用し、`tail_calls_optimized` を増やす
  - 観測上は `tail_calls_optimized` が増える

つまり、「source 上で末尾っぽく見えるか」よりも、「その位置が最終的に `Call -> Return` へ lower されるか」が実際の判定基準です。

## 確認方法

`surtr run --vm-stats ...` で `tail_calls_optimized` と `max_frame_depth` を見ると確認しやすいです。

見るポイント:

- tail-recursive な関数では `tail_calls_optimized` が増える
- `return_count` は非最適化時より小さくなりうる
- `max_frame_depth` は入力サイズに比例して伸びにくくなる
- non-tail recursion では `tail_calls_optimized == 0` のまま

## いまの割り切り

- source 上の末尾位置は「現在の関数 / closure / extractor / process handler の返り値そのものになる式位置」として扱う
- `Facet` は compile-time-only なので runtime TCO 対象ではない。ただし Facet API に渡す closure の body 内 tail call は通常の closure TCO と同じ
- process / task / worker の hidden builtin、`Process::sleep`、IO は user-function TCO ではなく scheduler / runtime effect の契約で扱う
- 将来、明示的な tail marker や別 lowering を入れたら、この文書も更新が必要
