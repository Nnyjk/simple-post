/** 「导出日志」的返回体。字段与 Rust 的 `log_api::LogExport`（camelCase）逐字一致。 */
export interface LogExport {
  /** 纯文本，一行一条记录；本次运行没有匹配的日志时为空串。 */
  content: string;
  /** 建议的文件名：`simple-post-logs-<YYYYMMDD-HHMMSS>.txt` 或 `simple-post-errors-<…>.txt`。 */
  fileName: string;
}
