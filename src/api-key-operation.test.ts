import { test } from "node:test";
import assert from "node:assert/strict";
import {
  beginApiKeyOperation,
  isCurrentApiKeyOperation,
} from "./api-key-operation.ts";

test("Key操作跨设置页面生命周期：清除或新保存拒绝旧响应", () => {
  const saving = beginApiKeyOperation();
  const clearing = beginApiKeyOperation();
  assert.equal(isCurrentApiKeyOperation(saving), false);
  assert.equal(isCurrentApiKeyOperation(clearing), true);
  const newerSave = beginApiKeyOperation();
  assert.equal(isCurrentApiKeyOperation(clearing), false);
  assert.equal(isCurrentApiKeyOperation(newerSave), true);
});
