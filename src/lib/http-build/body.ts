import type { RequestBody } from '@/types/domain';

/** 仅当 mode 允许且内容非空时才附上 body。 */
export function hasBody(body: RequestBody): boolean {
  return body.mode !== 'none' && body.content.length > 0;
}
