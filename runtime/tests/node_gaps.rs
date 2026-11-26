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
fn diagnostics_channel_exists() {
    let r = run(&[("a.test.ts", "
        import dc, { channel, hasSubscribers, tracingChannel } from 'node:diagnostics_channel';
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
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (1, 0), "{:?}", r.error);
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

#[test]
fn path_win32_follows_windows_rules() {
    let r = run(&[("a.test.ts", "
        import path from 'node:path';
        import win32 from 'node:path/win32';
        import { win32 as w2 } from 'node:path';
        test('win32', () => {
          expect(win32).toBe(path.win32);
          expect(w2.sep).toBe('\\\\');
          expect(win32.isAbsolute('C:\\\\x')).toBe(true);
          expect(win32.isAbsolute('/x')).toBe(true);
          expect(win32.isAbsolute('x')).toBe(false);
          expect(win32.join('C:/Users', 'Ada', '..', 'code')).toBe('C:\\\\Users\\\\code');
          expect(win32.resolve('C:\\\\a', '..', 'b')).toBe('C:\\\\b');
          expect(win32.parse('\\\\\\\\nas\\\\share\\\\proj').root).toBe('\\\\\\\\nas\\\\share\\\\');
          expect(win32.basename('C:\\\\a\\\\b.txt', '.txt')).toBe('b');
          expect(path.sep).toBe('/');
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (1, 0), "{:?}", r.error);
}

#[test]
fn fs_descriptors_stats_and_links_are_real() {
    let r = run(&[("a.test.ts", "
        import fs, { openSync, readSync, writeSync, closeSync, fstatSync, lstatSync, statSync, realpathSync, symlinkSync, writeFileSync, readFileSync, mkdirSync, rmSync, readdirSync, Stats, Dirent, constants } from 'node:fs';
        import { open, lstat, stat, readdir } from 'node:fs/promises';
        import { tmpdir } from 'node:os';
        const dir = tmpdir() + '/xmas-fs-' + Date.now() + '-' + Math.floor(Math.random() * 1e6);
        mkdirSync(dir);
        test('binary read and write through a descriptor', () => {
          const file = dir + '/bin';
          const out = openSync(file, 'w');
          expect(writeSync(out, new Uint8Array([0, 255, 128, 10]))).toBe(4);
          expect(writeSync(out, 'hi')).toBe(2);
          closeSync(out);
          const fd = openSync(file, 'r');
          const buf = new Uint8Array(6);
          expect(readSync(fd, buf, 0, 6, null)).toBe(6);
          expect([...buf]).toEqual([0, 255, 128, 10, 104, 105]);
          expect(readSync(fd, buf, 0, 6, null)).toBe(0);
          expect(readSync(fd, buf, 0, 2, 4)).toBe(2);
          expect(fstatSync(fd).size).toBe(6);
          closeSync(fd);
        });
        test('append mode appends and w truncates', () => {
          const file = dir + '/ap';
          writeFileSync(file, 'abc');
          const a = openSync(file, 'a'); writeSync(a, 'def'); closeSync(a);
          expect(readFileSync(file)).toBe('abcdef');
          const w = openSync(file, 'w'); closeSync(w);
          expect(statSync(file).size).toBe(0);
          expect(() => openSync(dir + '/nope', 'r')).toThrow();
        });
        test('lstat sees a link, stat follows it, realpath resolves it', async () => {
          writeFileSync(dir + '/target', 'x');
          symlinkSync(dir + '/target', dir + '/link');
          expect(lstatSync(dir + '/link').isSymbolicLink()).toBe(true);
          expect(statSync(dir + '/link').isSymbolicLink()).toBe(false);
          expect(statSync(dir + '/link').isFile()).toBe(true);
          expect((await lstat(dir + '/link')).isSymbolicLink()).toBe(true);
          expect((await stat(dir + '/link')).isFile()).toBe(true);
          expect(realpathSync(dir + '/link')).toBe(realpathSync(dir + '/target'));
          expect(lstatSync(dir + '/missing', { throwIfNoEntry: false })).toBe(undefined);
        });
        test('Stats carry the fields Node gives them', () => {
          const s = statSync(dir);
          expect(s instanceof Stats).toBe(true);
          expect(s.isDirectory()).toBe(true);
          for (const k of ['dev', 'ino', 'mode', 'nlink', 'uid', 'gid', 'size', 'blksize', 'blocks', 'atimeMs', 'mtimeMs', 'ctimeMs', 'birthtimeMs']) expect(typeof s[k]).toBe('number');
          expect(s.mtime instanceof Date).toBe(true);
          expect(s.ino).toBeGreaterThan(0);
          expect(constants.O_CREAT).toBeGreaterThan(0);
        });
        test('Dirent and FileHandle', async () => {
          const entries = readdirSync(dir, { withFileTypes: true });
          expect(entries[0] instanceof Dirent).toBe(true);
          const link = entries.find((e) => e.name === 'link');
          expect(link.isSymbolicLink()).toBe(true);
          expect(link.isFile()).toBe(false);
          expect(link.parentPath).toBe(dir);
          expect((await readdir(dir, { withFileTypes: true })).length).toBe(entries.length);
          const h = await open(dir + '/bin', 'r');
          const buf = new Uint8Array(4);
          const { bytesRead } = await h.read(buf, 0, 4, 0);
          expect(bytesRead).toBe(4);
          expect([...buf]).toEqual([0, 255, 128, 10]);
          expect((await h.stat()).size).toBe(6);
          await h.close();
          rmSync(dir, { recursive: true, force: true });
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (5, 0), "{:?}", r.error);
}

#[test]
fn os_signals_come_from_the_platform() {
    let r = run(&[("a.test.ts", "
        import os from 'node:os';
        test('signals', () => {
          expect(os.constants.signals.SIGTERM).toBe(15);
          expect(os.constants.signals.SIGKILL).toBe(9);
          expect(os.constants.signals.SIGINT).toBe(2);
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (1, 0), "{:?}", r.error);
}

#[test]
fn fs_callbacks_and_promises_both_work() {
    let r = run(&[("a.test.ts", "
        import fs, { readdir, stat, readFile } from 'node:fs';
        import { readdir as readdirP } from 'node:fs/promises';
        import { fileURLToPath } from 'node:url';
        const here = fileURLToPath(new URL('.', import.meta.url));
        test('callback form', async () => {
          const names = await new Promise((resolve, reject) => readdir(here, (e, v) => e ? reject(e) : resolve(v)));
          expect(names).toContain('a.test.ts');
          const entries = await new Promise((resolve, reject) => fs.readdir(here, { withFileTypes: true }, (e, v) => e ? reject(e) : resolve(v)));
          expect(entries[0].isFile()).toBe(true);
          const err = await new Promise((resolve) => stat('/nope-xmas', (e) => resolve(e)));
          expect(err.code).toBe('ENOENT');
        });
        test('promise form still works', async () => {
          expect(await readdirP(here)).toContain('a.test.ts');
          expect(typeof (await readFile(here + 'a.test.ts', 'utf8'))).toBe('string');
          expect(fs.promises.readdir).toBe(readdirP);
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (2, 0), "{:?}", r.error);
}

#[test]
fn child_process_sends_the_signal_it_is_given() {
    let r = run(&[("a.test.ts", "
        import { spawn } from 'node:child_process';
        test('SIGTERM ends a sleeping child and the close event says which signal', async () => {
          const child = spawn('sleep', ['30']);
          const closed = new Promise((resolve) => child.on('close', (code, signal) => resolve({ code, signal })));
          expect(child.kill('SIGTERM')).toBe(true);
          const { code, signal } = await closed;
          expect(code === null || code !== 0).toBe(true);
          expect(signal === 'SIGTERM' || signal === null).toBe(true);
        });
        test('an unknown signal name throws', () => {
          const child = spawn('sleep', ['30']);
          expect(() => child.kill('SIGNOPE')).toThrow();
          child.kill('SIGKILL');
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (2, 0), "{:?}", r.error);
}

#[test]
fn spawn_of_a_missing_program_has_streams_and_a_code() {
    let r = run(&[("a.test.ts", "
        import { spawn } from 'node:child_process';
        test('missing program', async () => {
          const child = spawn('/no/such/program-xmas', ['a']);
          expect(typeof child.stdout.on).toBe('function');
          expect(typeof child.stderr.on).toBe('function');
          const error: any = await new Promise((resolve) => child.on('error', resolve));
          expect(error.code).toBe('ENOENT');
          expect(error.path).toBe('/no/such/program-xmas');
          expect(error.message).toBe('spawn /no/such/program-xmas ENOENT');
          expect(String(error.stack).startsWith('Error: spawn /no/such/program-xmas ENOENT')).toBe(true);
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (1, 0), "{:?}", r.error);
}

#[test]
fn child_stderr_delivers_every_chunk_while_the_child_runs() {
    let r = run(&[
        ("fake.sh", "#!/bin/sh\nprintf 'a\\n' >&2; sleep 0.1\nprintf 'b\\n' >&2; sleep 0.1\nprintf 'c\\n' >&2\n"),
        ("a.test.ts", "
        import { spawn } from 'node:child_process';
        import { fileURLToPath } from 'node:url';
        test('chunks arrive before exit', async () => {
          const script = fileURLToPath(new URL('./fake.sh', import.meta.url));
          const child = spawn('sh', [script]);
          const seen: string[] = [];
          let exited = false;
          child.stderr.on('data', (c) => seen.push((exited ? 'late:' : '') + String(c).trim()));
          child.on('exit', () => { exited = true; });
          await new Promise((resolve) => child.on('close', resolve));
          expect(seen).toEqual(['a', 'b', 'c']);
        });
    ")], "a.test.ts");
    assert_eq!((r.pass, r.fail), (1, 0), "{:?}", r.error);
}
