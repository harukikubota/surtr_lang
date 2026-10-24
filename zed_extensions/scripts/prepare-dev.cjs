const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

const root = path.resolve(__dirname, '..');
const grammar = path.join(root, 'dist/grammar-source');
const extension = path.join(root, 'dist/surtr');
fs.mkdirSync(grammar, { recursive: true });
fs.cpSync(path.join(root, 'tree-sitter-surtr/src'), path.join(grammar, 'src'), { recursive: true });

function git(args, options = {}) {
  return execFileSync('git', args, {
    cwd: grammar, encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env,
      GIT_AUTHOR_NAME: 'Surtr grammar build', GIT_AUTHOR_EMAIL: 'build@localhost',
      GIT_COMMITTER_NAME: 'Surtr grammar build', GIT_COMMITTER_EMAIL: 'build@localhost',
      GIT_AUTHOR_DATE: '2000-01-01T00:00:00Z', GIT_COMMITTER_DATE: '2000-01-01T00:00:00Z'
    }, ...options
  }).trim();
}

// Zed fetches a Git revision even for file:// grammars. This disposable build
// repository makes uncommitted grammar sources available without committing
// anything in the Surtr checkout or relying on a published grammar repository.
if (!fs.existsSync(path.join(grammar, '.git'))) git(['init', '--quiet']);
git(['add', '--all', '--', 'src']);
const tree = git(['write-tree']);
const rev = git(['commit-tree', tree], { input: 'Generated Surtr grammar\n' });
git(['update-ref', 'refs/heads/generated', rev]);
fs.mkdirSync(extension, { recursive: true });
fs.cpSync(path.join(root, 'surtr/languages'), path.join(extension, 'languages'), { recursive: true });
const metadata = fs.readFileSync(path.join(root, 'surtr/extension.toml'), 'utf8');
fs.writeFileSync(path.join(extension, 'extension.toml'), metadata +
  `\n[grammars.surtr]\nrepository = ${JSON.stringify(pathToFileURL(grammar).href)}\nrev = "${rev}"\n`);
console.log(`Zed: run "zed: install dev extension" and select:\n${extension}`);
