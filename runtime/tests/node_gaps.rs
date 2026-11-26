//! Gaps found running a real monorepo: node:fs sync names and streams, diagnostics_channel, v8,
//! and workspace package resolution.
mod common;

use common::tree;
use xmas_runtime::host::Host;
use xmas_runtime::testrun::{run_file, Options};

fn run(files: &[(&str, &str)], entry: &str) -> xmas_runtime::testrun::FileResult {
    let dir = tree(files);
    run_file(&dir.join(entry), &Options::default(), Host::capture())
}

#[test]
fn fs_has_named_sync_exports_and_streams() {
    let r = run(&[("a.test.ts", "
        import fs, { readFileSync, writeFileSync, existsSync, createWriteStream, createReadStream } from 'node:fs';
        import { tmpdir } from 'node:os';
        test('names', async () => {
          expect(typeof readFileSync).toBe('function');
          expect(fs.readFileSync).toBe(readFileSync);
          const file = tmpdir() + '/xmas-gap-' + Date.now() + '.txt';
          const out = createWriteStream(file);
          out.write('hello ');
          await new Promise((resolve) => out.end('world', resolve));
          expect(existsSync(file)).toBe(true);
          expect(readFileSync(file)).toBe('hello world');
          let text = '';
          for await (const chunk of createReadStream(file)) text += chunk;
          expect(text).toBe('hello world');
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (1, 0), "{:?}", r.error);
}

#[test]
fn diagnostics_channel_and_v8_exist() {
    let r = run(&[("a.test.ts", "
        import dc, { channel, hasSubscribers, tracingChannel } from 'node:diagnostics_channel';
        import v8 from 'node:v8';
        test('dc', () => {
          expect(hasSubscribers('x')).toBe(false);
          const seen: unknown[] = [];
          channel('x').subscribe((m) => seen.push(m));
          expect(hasSubscribers('x')).toBe(true);
          channel('x').publish(1);
          expect(seen).toEqual([1]);
          const t = tracingChannel('t');
          expect(t.traceSync(() => 5)).toBe(5);
          expect(dc.channel).toBe(channel);
        });
        test('v8', () => {
          expect(v8.deserialize(v8.serialize({ a: [1, 2] }))).toEqual({ a: [1, 2] });
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (2, 0), "{:?}", r.error);
}

#[test]
fn workspace_packages_resolve_without_node_modules() {
    let r = run(&[
        ("package.json", r#"{"workspaces":["packages/*"]}"#),
        ("packages/lib/package.json", r#"{"name":"@scope/lib","main":"./src/index.ts"}"#),
        ("packages/lib/src/index.ts", "export const lib = 'lib'; export * from './more';"),
        ("packages/lib/src/more.ts", "export const more = 'more';"),
        ("packages/lib/src/deep/x.ts", "export const x = 'x';"),
        ("packages/app/package.json", r#"{"name":"@scope/app"}"#),
        ("packages/app/a.test.ts", "
            import { lib, more } from '@scope/lib';
            import { x } from '@scope/lib/src/deep/x';
            test('ws', () => { expect([lib, more, x]).toEqual(['lib', 'more', 'x']); });
        "),
    ], "packages/app/a.test.ts");
    assert_eq!((r.pass, r.fail), (1, 0), "{:?}", r.error);
}
