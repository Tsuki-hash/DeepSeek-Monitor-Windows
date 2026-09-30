import { test } from "node:test";
import assert from "node:assert/strict";
import { updateTime } from "./data-display.ts";

test("更新时间默认使用电脑本地时区", () => {
  const timestamp = Date.parse("2026-09-30T16:05:06Z");
  const localTimeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  assert.equal(
    updateTime(timestamp),
    updateTime(timestamp, false, localTimeZone),
  );
  assert.equal(
    updateTime(timestamp, true),
    updateTime(timestamp, true, localTimeZone),
  );
});

test("跨日更新时间随时区变化，平台记账日期不受影响", () => {
  const timestamp = Date.parse("2026-09-30T16:05:06Z");
  assert.equal(updateTime(timestamp, true, "Asia/Shanghai"), "10/01 00:05:06");
  assert.equal(
    updateTime(timestamp, true, "America/New_York"),
    "09/30 12:05:06",
  );
});

test("未取得成功快照时不伪造更新时间", () => {
  assert.equal(updateTime(null), "尚未更新");
  assert.equal(updateTime(undefined), "尚未更新");
});
