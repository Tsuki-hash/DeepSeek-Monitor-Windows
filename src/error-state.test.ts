import { test } from "node:test";
import assert from "node:assert/strict";
import { errorInfo } from "./error-state.ts";
test("IPC 错误码独立于显示语言，旧字符串与 Error 仍能显示", () => {
  assert.equal(
    errorInfo({
      code: "not_configured",
      message: "Credential missing",
      retryable: false,
    }).code,
    "not_configured",
  );
  assert.equal(errorInfo("未配置用量 Token").code, "not_configured");
  assert.equal(
    errorInfo(new Error("disk unavailable")).message,
    "disk unavailable",
  );
});
