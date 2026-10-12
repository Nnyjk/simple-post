import { Cloud, Rocket, TestTube } from 'lucide-react';

/**
 * Pick an icon for the env chip based on its name. The mapping is loose
 * by design — names that don't match anything get the neutral Cloud
 * icon, which still differentiates the env visually without forcing the
 * user to pick an icon manually.
 *
 * `rows/EnvSwitcher` is the only caller left: the env body no longer has a
 * name row of its own, so the switcher owns both the name and its glyph.
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
