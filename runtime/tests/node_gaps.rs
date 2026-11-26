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

#[test]
fn events_timers_module_and_fs_descriptors() {
    let r = run(&[("a.test.ts", "
        import { EventEmitter, once, addAbortListener } from 'node:events';
        import { scheduler, setTimeout as sleep } from 'node:timers/promises';
        import { createRequire, isBuiltin } from 'node:module';
        import { openSync, readSync, closeSync, writeFileSync, rmSync, existsSync } from 'node:fs';
        import { tmpdir } from 'node:os';
        import { execArgv } from 'node:process';
        test('once', async () => {
          const e = new EventEmitter();
          setTimeout(() => e.emit('x', 1, 2), 1);
          expect(await once(e, 'x')).toEqual([1, 2]);
          expect(typeof addAbortListener).toBe('function');
        });
        test('timers', async () => {
          await scheduler.wait(1);
          expect(await sleep(1, 'v')).toBe('v');
          expect(Array.isArray(execArgv)).toBe(true);
        });
        test('module', () => {
          expect(isBuiltin('node:fs')).toBe(true);
          expect(typeof createRequire('/x')).toBe('function');
        });
        test('fd', () => {
          const file = tmpdir() + '/xmas-fd-' + Date.now();
          writeFileSync(file, 'abcdef');
          const fd = openSync(file, 'r');
          const buf = new Uint8Array(3);
          expect(readSync(fd, buf, 0, 3, 1)).toBe(3);
          expect(String.fromCharCode(...buf)).toBe('bcd');
          closeSync(fd);
          rmSync(file);
          expect(existsSync(file)).toBe(false);
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (4, 0), "{:?}", r.error);
}

#[test]
fn wildcard_exports_and_commonjs_reexports() {
    let r = run(&[
        ("a.test.ts", "
            import { wild } from 'pkg/utils/thing';
            import { inner, other } from 'cjs';
            test('x', () => { expect([wild, inner, other]).toEqual(['w', 'i', 'o']); });
        "),
        ("node_modules/pkg/package.json", r#"{"name":"pkg","exports":{"./utils/*":{"import":"./dist/utils/*.js"}}}"#),
        ("node_modules/pkg/dist/utils/thing.js", "export const wild = 'w';"),
        ("node_modules/cjs/package.json", r#"{"name":"cjs","main":"./index.js"}"#),
        ("node_modules/cjs/index.js", "function __export(m){for(var p in m) if(!exports.hasOwnProperty(p)) exports[p]=m[p];}\nObject.defineProperty(exports,'__esModule',{value:true});\n__export(require('./src'));\nexports.other = 'o';"),
        ("node_modules/cjs/src.js", "exports.inner = 'i';"),
    ], "a.test.ts");
    assert_eq!((r.pass, r.fail), (1, 0), "{:?}", r.error);
}
