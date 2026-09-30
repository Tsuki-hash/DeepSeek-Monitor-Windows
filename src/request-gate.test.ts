import { test } from "node:test";
import assert from "node:assert/strict";
import { RequestGate } from "./request-gate.ts";

test("RequestGate：清除后旧请求不能提交，旧请求结束不释放新请求", async () => {
  const gate = new RequestGate<number>();
  let resolveOld: ((n: number) => void) | undefined;
  let resolveNew: ((n: number) => void) | undefined;
  const old = gate.run(
    () =>
      new Promise<number>((resolve) => {
        resolveOld = resolve;
      }),
  );
  await Promise.resolve();
  gate.invalidate();
  const fresh = gate.run(
    () =>
      new Promise<number>((resolve) => {
        resolveNew = resolve;
      }),
  );
  await Promise.resolve();
  assert.ok(resolveOld);
  resolveOld(1);
  await assert.rejects(() => old);
  assert.equal(
    gate.run(() => Promise.resolve(3)),
    fresh,
  );
  assert.ok(resolveNew);
  resolveNew(2);
  assert.equal(await fresh, 2);
});

test("RequestGate：并发查询共享请求，失败之后可以重试", async () => {
  const gate = new RequestGate<number>();
  let calls = 0;
  const first = gate.run(async () => {
    calls += 1;
    throw new Error("offline");
  });
  assert.equal(
    gate.run(() => Promise.resolve(2)),
    first,
  );
  await assert.rejects(() => first);
  assert.equal(calls, 1);
  assert.equal(await gate.run(() => Promise.resolve(4)), 4);
});
