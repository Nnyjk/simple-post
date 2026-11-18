import { Select } from '@/components/ui/select';
import { REQUEST_PROTOCOLS } from '@/types/domain';
import type { RequestProtocol } from '@/types/domain';

/** 协议选择器上给用户看的名字；线上值就是 `RequestProtocol` 本身。 */
const PROTOCOL_LABEL: Record<RequestProtocol, string> = {
  http: 'HTTP',
  graphql: 'GraphQL',
  sse: 'SSE',
  websocket: 'WebSocket',
};

/**
 * 请求协议下拉。它是端点的一个属性，与 method 一样写草稿、随保存落盘。
 *
 * 放在 URL 之前：同一个地址用 HTTP 取一次、再用 SSE 订阅一次是两件不同的事，用户要先说清
 * 是哪一件，方法 / 地址栏才读得对。
 */
export function ProtocolPicker({
  value,
  onChange,
  className,
}: {
  value: RequestProtocol;
  onChange: (next: RequestProtocol) => void;
  className?: string;
}) {
  return (
    <Select
      aria-label="请求协议"
      title="请求协议：决定请求怎么发、响应怎么显示"
      value={value}
      onChange={(e) => onChange(e.target.value as RequestProtocol)}
      options={REQUEST_PROTOCOLS.map((p) => ({ value: p, label: PROTOCOL_LABEL[p] }))}
      className={className}
      data-testid="protocol-picker"
    />
  );
}
