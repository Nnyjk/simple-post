/** 「导出日志」的返回体。字段与 Rust 的 `log_api::LogExport`（camelCase）逐字一致。 */
export interface LogExport {
  /** 纯文本，一行一条记录；本次运行没有匹配的日志时为空串。 */
  content: string;
  /** 建议的文件名：`simple-post-logs-<YYYYMMDD-HHMMSS>.txt` 或 `simple-post-errors-<…>.txt`。 */
  fileName: string;
}

/**
 * 桌面端落盘结果。字段与 Rust 的 `log_api::LogExportFile` 一致。
 *
 * 只有桌面壳的 `log_export_file` 返回它：日志写进 `<data_dir>/simple-post/logs/`，
 * `path` 是那个文件的完整路径。浏览器形态走下载，没有这个对象。
 */
export interface LogExportFile {
  /** 与 `LogExport.fileName` 同一条规则。 */
  fileName: string;
  /** 落盘文件的完整路径。 */
  path: string;
}
