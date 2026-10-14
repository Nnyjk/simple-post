import { Cloud, Rocket, TestTube } from 'lucide-react';

/**
 * 按环境名挑一个图标。映射刻意宽松 —— 匹配不上任何规则的名字拿到中性的 Cloud 图标，
 * 仍能在视觉上区分环境，而不必强迫用户手动挑图标。
 *
 * `rows/EnvSwitcher` 是如今唯一的调用方：环境内容不再有名字行，故名字与其字形都由切换
 * 器持有。
 */
export function envIcon(name: string) {
  const n = name.toLowerCase();
  if (n.includes('prod') || n.includes('release') || n.includes('正式') || n.includes('线上')) {
    return <Rocket className="h-3.5 w-3.5" />;
  }
  if (n.includes('stag') || n.includes('pre') || n.includes('测试') || n.includes('预发')) {
    return <TestTube className="h-3.5 w-3.5" />;
  }
  if (n.includes('dev') || n.includes('local') || n.includes('开发') || n.includes('本地')) {
    return <TestTube className="h-3.5 w-3.5" />;
  }
  return <Cloud className="h-3.5 w-3.5" />;
}
