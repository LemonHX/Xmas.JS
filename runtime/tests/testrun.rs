//! `xmas test`: file discovery and the test interface, end to end.
mod common;

use common::tree;
use xmas_runtime::host::Host;
use xmas_runtime::testrun::{find_tests, run_file, Options};

fn result(body: &str) -> xmas_runtime::testrun::FileResult {
    let dir = tree(&[("a.test.ts", body)]);
    run_file(&dir.join("a.test.ts"), &Options::default(), Host::capture())
}

#[test]
fn finds_test_files_by_name_and_skips_node_modules() {
    let dir = tree(&[
        ("a.test.ts", ""),
        ("sub/b_test.js", ""),
        ("c.spec.tsx", ""),
        ("plain.ts", ""),
        ("node_modules/x/d.test.js", ""),
    ]);
    let names: Vec<String> = find_tests(&dir, &[])
        .iter()
        .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["a.test.ts", "c.spec.tsx", "sub/b_test.js"]);
    assert_eq!(find_tests(&dir, &["sub".into()]).len(), 1);
}

#[test]
fn passing_failing_skipped_and_todo_are_counted() {
    let r = result(
        "import { test, expect } from 'xmas:test';
         test('ok', () => { expect(1).toBe(1); });
         test('bad', () => { expect(1).toBe(2); });
         test.skip('skipped', () => {});
         test.todo('later');",
    );
    assert_eq!((r.pass, r.fail, r.skip, r.todo, r.expects), (1, 1, 1, 1, 2));
    assert!(r.error.is_none());
}

#[test]
fn hooks_run_in_order_around_each_test() {
    let r = result(
        "import { describe, test, expect, beforeAll, beforeEach, afterEach, afterAll } from 'xmas:test';
         const log: string[] = [];
         describe('s', () => {
           beforeAll(() => { log.push('ba'); });
           beforeEach(() => { log.push('be'); });
           afterEach(() => { log.push('ae'); });
           afterAll(() => { log.push('aa'); });
           test('one', () => { log.push('1'); });
           test('two', () => { log.push('2'); });
         });
         test('after', () => { expect(log.join(',')).toBe('ba,be,1,ae,be,2,ae,aa'); });",
    );
    assert_eq!((r.pass, r.fail), (3, 0));
}

#[test]
fn matchers_and_negation() {
    let r = result(
        "import { test, expect } from 'xmas:test';
         test('m', () => {
           expect({ a: [1, { b: 2 }] }).toEqual({ a: [1, { b: 2 }] });
           expect({ a: 1, b: undefined }).toEqual({ a: 1 });
           expect({ a: 1, b: undefined }).not.toStrictEqual({ a: 1 });
           expect({ a: 1, b: 2 }).toMatchObject({ a: 1 });
           expect([1, 2, 3]).toContain(2);
           expect('abc').toMatch(/b/);
           expect(0.1 + 0.2).toBeCloseTo(0.3);
           expect(() => { throw new TypeError('boom'); }).toThrow('boom');
           expect(() => { throw new TypeError('boom'); }).toThrow(TypeError);
           expect({ a: { b: 1 } }).toHaveProperty('a.b', 1);
           expect(new Map([[1, 2]])).toEqual(new Map([[1, 2]]));
           expect(3).toBeGreaterThan(2);
           expect(1).not.toBe(2);
           expect({ n: 1, s: 'x' }).toEqual({ n: expect.any(Number), s: expect.stringContaining('x') });
         });
         test('a failing matcher throws', () => {
           expect(() => expect(1).toBe(2)).toThrow('Expected: 2');
           expect(() => expect(1).not.toBe(1)).toThrow('Expected: not 1');
         });",
    );
    assert_eq!((r.pass, r.fail), (2, 0));
}

#[test]
fn promises_resolves_rejects_and_done_callbacks() {
    let r = result(
        "import { test, expect } from 'xmas:test';
         test('resolves', async () => { await expect(Promise.resolve(5)).resolves.toBe(5); });
         test('rejects', async () => { await expect(Promise.reject(new Error('x'))).rejects.toThrow('x'); });
         test('done', (done) => { setTimeout(() => done(), 1); });
         test('each %s', () => {});
         test.each([[1, 2], [3, 4]])('sum %d %d', (a, b) => { expect(b - a).toBe(1); });",
    );
    assert_eq!((r.pass, r.fail), (6, 0));
}

#[test]
fn mocks_and_spies_record_and_restore() {
    let r = result(
        "import { test, expect, mock, spyOn } from 'xmas:test';
         test('mock', () => {
           const f = mock((x: number) => x * 2);
           f(2); f(3);
           expect(f).toHaveBeenCalledTimes(2);
           expect(f).toHaveBeenCalledWith(3);
           expect(f.mock.results[0].value).toBe(4);
           f.mockClear();
           expect(f).not.toHaveBeenCalled();
         });
         test('spy', () => {
           const o = { m() { return 1; } };
           const s = spyOn(o, 'm').mockReturnValue(9);
           expect(o.m()).toBe(9);
           s.mockRestore();
           expect(o.m()).toBe(1);
         });",
    );
    assert_eq!((r.pass, r.fail), (2, 0));
}

#[test]
fn a_test_that_never_settles_times_out() {
    let dir = tree(&[(
        "a.test.ts",
        "import { test } from 'xmas:test'; test('hang', async () => { await new Promise(() => {}); }, 30);",
    )]);
    let r = run_file(&dir.join("a.test.ts"), &Options::default(), Host::capture());
    assert_eq!((r.pass, r.fail), (0, 1));
}

#[test]
fn only_and_the_name_pattern_select_tests() {
    let r = result(
        "import { test } from 'xmas:test';
         test('a', () => {});
         test.only('b', () => {});",
    );
    assert_eq!((r.pass, r.fail), (1, 0));
    let dir = tree(&[("a.test.ts", "import { test } from 'xmas:test'; test('keep me', () => {}); test('drop', () => { throw new Error('x'); });")]);
    let options = Options { name_pattern: Some("keep".into()), ..Options::default() };
    let r = run_file(&dir.join("a.test.ts"), &options, Host::capture());
    assert_eq!((r.pass, r.fail), (1, 0));
}

#[test]
fn globals_work_without_an_import_and_a_broken_file_reports_an_error() {
    let r = result("test('g', () => { expect(1).toBe(1); });");
    assert_eq!((r.pass, r.fail), (1, 0));
    let r = result("this is a syntax error(");
    assert!(r.error.is_some());
}
