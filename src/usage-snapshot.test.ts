import { test } from "node:test";
import assert from "node:assert/strict";
import { currentUsageSnapshot } from "./usage-snapshot.ts";
import { accountingDateKey, emptyUsageDay } from "./format.ts";
import type { UsageResult } from "./types";
const snapshot: UsageResult = {
  accountingDate: "2026-09-30",
  models: [],
  days: [],
  monthCost: 43.67,
};

test("同月失败保留快照，跨月和跨年拒绝旧累计", () => {
  assert.equal(currentUsageSnapshot(snapshot, "2026-09-30"), snapshot);
  assert.equal(currentUsageSnapshot(snapshot, "2026-10-01"), null);
  assert.equal(
    currentUsageSnapshot(
      { ...snapshot, accountingDate: "2026-12-31" },
      "2027-01-01",
    ),
    null,
  );
  assert.equal(
    currentUsageSnapshot(
      { ...snapshot, accountingDate: undefined },
      "2026-09-30",
    ),
    null,
  );
});

test("同月跨日未获取日期标为未知，并保留月初上月缺失", () => {
  const prior: UsageResult = {
    ...snapshot,
    accountingDate: "2026-10-01",
    unavailableDates: ["2026-09-29", "2026-09-30"],
  };
  const retained = currentUsageSnapshot(prior, "2026-10-03");
  assert.deepEqual(retained?.unavailableDates, [
    "2026-09-29",
    "2026-09-30",
    "2026-10-03",
    "2026-10-02",
  ]);
  assert.equal(retained?.monthCost, 43.67);
  assert.deepEqual(prior.unavailableDates, ["2026-09-29", "2026-09-30"]);
});

test("东八区月份边界与电脑时区无关", () => {
  assert.equal(
    currentUsageSnapshot(
      snapshot,
      accountingDateKey(new Date("2026-09-30T15:59:59Z")),
    ),
    snapshot,
  );
  assert.equal(
    currentUsageSnapshot(
      snapshot,
      accountingDateKey(new Date("2026-09-30T16:00:00Z")),
    ),
    null,
  );
});

test("快照的未来零占位不能覆盖未知日期", () => {
  const prior = {
    ...snapshot,
    accountingDate: "2026-09-28",
    days: [emptyUsageDay("2026-09-29"), emptyUsageDay("2026-09-30")],
  };
  const retained = currentUsageSnapshot(prior, "2026-09-30");
  assert.ok(retained);
  assert.equal(retained.days.length, 0);
  assert.ok(retained.unavailableDates?.includes("2026-09-30"));
});
