// `node:*` 内置模块的最小类型声明。
//
// 只为测试文件服务，避免为了跑单测而引入 @types/node（那会给项目加一条 devDependency，
// 且它的全局声明会污染应用侧的 DOM 类型环境）。这里只声明 format.test.ts 实际用到的两个模块。

declare module "node:test" {
  export interface TestContext {
    /** 跳过当前子测试。 */
    skip(message?: string): void;
    /** 标记当前子测试为待办。 */
    todo(message?: string): void;
    /** 显式结束异步子测试。 */
    done(error?: unknown): void;
  }

  export type TestFn = (context: TestContext) => void | Promise<void>;

  export function test(name: string, fn: TestFn): Promise<void>;
  export function test(
    name: string,
    options: { skip?: boolean },
    fn: TestFn,
  ): Promise<void>;
  export function describe(name: string, fn: () => void): void;
  export function it(name: string, fn: TestFn): Promise<void>;
}

declare module "node:assert/strict" {
  interface Assert {
    (value: unknown, message?: string): asserts value;
    ok(value: unknown, message?: string): asserts value;
    equal<T>(actual: T, expected: T, message?: string): void;
    notEqual(actual: unknown, expected: unknown, message?: string): void;
    deepEqual<T>(actual: T, expected: T, message?: string): void;
    deepStrictEqual<T>(actual: T, expected: T, message?: string): void;
    strictEqual<T>(actual: T, expected: T, message?: string): void;
    notStrictEqual(actual: unknown, expected: unknown, message?: string): void;
    throws(fn: () => unknown, message?: string): void;
    rejects(fn: () => Promise<unknown>, message?: string): Promise<void>;
  }
  const assert: Assert;
  export default assert;
}
