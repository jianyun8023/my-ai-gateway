// Build an immutable, repository-local load toolchain. Requires Go on PATH.
import { readFileSync, mkdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const versions = JSON.parse(readFileSync(resolve(root, 'tests/tooling/versions.json')));
const bin = resolve(root, 'target/tools');
mkdirSync(bin, { recursive: true });
const env = { ...process.env, GOBIN: bin, GOPATH: process.env.GOPATH || resolve(root, 'target/tools/go'), GOCACHE: process.env.GOCACHE || resolve(root, 'target/tools/go-cache') };
for (const [command, args] of [
  ['go', ['install', `go.k6.io/xk6/cmd/xk6@v${versions.xk6}`]],
  [resolve(bin, 'xk6'), ['build', `v${versions.k6}`, '--with', `github.com/phymbert/xk6-sse@v${versions.xk6_sse}`, '--output', resolve(bin, 'k6')]],
  [resolve(bin, 'k6'), ['version']],
]) {
  const result = spawnSync(command, args, { cwd: root, env, stdio: 'inherit' });
  if (result.error || result.status !== 0) { console.error(result.error?.message || 'Load tool build failed'); process.exit(result.status || 1); }
}
