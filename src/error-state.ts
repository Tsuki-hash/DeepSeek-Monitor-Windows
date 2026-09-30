export type ErrorCode =
  | "not_configured"
  | "credentials_invalid"
  | "cancelled"
  | "rate_limited"
  | "encryption_unavailable"
  | "invalid_data"
  | "unavailable"
  | "unknown";
export function errorInfo(error: unknown): {
  code: ErrorCode;
  message: string;
  retryable: boolean;
} {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string"
  ) {
    const code =
      "code" in error && typeof error.code === "string"
        ? (error.code as ErrorCode)
        : "unknown";
    return {
      code,
      message: error.message,
      retryable: "retryable" in error && error.retryable === true,
    };
  }
  const message = typeof error === "string" ? error : "操作失败，请重试";
  return {
    code: message.includes("未配置") ? "not_configured" : "unknown",
    message,
    retryable: false,
  };
}
export const errorMessage = (error: unknown) => errorInfo(error).message;
