import {
  ArrowDown,
  ArrowRight,
  ArrowUp,
  ChevronDown,
  ChevronRight,
  FolderTree,
  GripVertical,
  Plus,
  Trash2,
} from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { InlineError } from '@/components/ui/inline-error';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { findShadowedRuleIndices, validateRewriteRules } from '@/lib/url';
import type { Environment, ResourceNode, RewriteRule } from '@/types/domain';
import type { EnvironmentSectionProps } from '../EnvironmentSection';
import {
  areRewriteListsEqual,
  cleanPersistableRewrites,
  resolveRewriteDropZone,
  reorderRewriteRules,
  type RewriteDropZone,
} from './rewrite-rules';

/** 没有规则时共用的空数组：`rulesOf` 的返回值必须是稳定引用，换序才判得出「没动过」。 */
const NO_RULES: RewriteRule[] = [];

/** 拖拽时写在 `DataTransfer` 上的私有类型，用来拒绝从别处拖进来的东西。 */
const DRAG_MIME = 'application/x-simple-post-rewrite';

/**
 * 在途拖拽的兜底下标。挂在模块作用域而不是 state 上：`drop` 事件里读 `DataTransfer`
 * 偶尔拿不到载荷（某些嵌入式 WebView 与自动化拖拽都这样），那时只能靠这份记录。
 */
let draggingRule: { moduleId: string; index: number } | null = null;

/**
 * 模块地址表 —— 每个模块一块：顶行是模块名、模块地址与路径改写入口，下面可展开的是这个
 * (环境 × 模块) 格子里的**有序**路径改写规则。
 *
 * 一条规则把端点**自己 URL** 的路径前缀从起点换成目标；模块地址里的主机与路径段一概不动。
 * 故模块地址是 `127.0.0.1:8080`、规则是 `/api/air → /api/project-a/air` 时，端点路径
 * `/api/air/xxxxx` 发出的是 `127.0.0.1:8080/api/project-a/air/xxxxx`。
 *
 * **顺序就是优先级**：按从上到下依次匹配，命中首条后不再向后匹配。故拖拽手柄、上移 / 下移
 * 按钮改的是行为，不只是排版。
 *
 * 写入分两次：顶行的模块地址每键入一个字符就写（沿用旧行为）；规则列表先进本地草稿，在
 * 失焦、回车、增删、换序时才原子提交。逐键入提交会在用户把 `/api/air` 补成 `/api/air/v2`
 * 的途中经过一个与其他规则重复的起点，被后端去重拒绝，输入随之回滚。
 *
 * 两种模式，一张表：`mode="project"` 是全项目视图，行集是本项目的全部模块，模块列出现；
 * `mode="module"` 是模块标签所示的单模块视图，模块列消失，规则区始终展开。
 */
interface ModuleBaseUrlTableProps {
  env: Environment;
  modules: readonly ResourceNode[];
  /** 本表服务的是哪个视图，见上方说明。 */
  mode?: 'project' | 'module';
  setEnvironmentModuleBaseUrl: EnvironmentSectionProps['setEnvironmentModuleBaseUrl'];
}

/** `mode="project"` 的顶行：模块名、模块地址、改写入口。 */
const TOP_GRID = 'grid grid-cols-[7rem_minmax(0,1fr)_auto] items-center gap-2';
/** `mode="module"` 去掉模块名列。 */
const TOP_GRID_SINGLE = 'grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2';
/** 一条规则里的起点、箭头与目标。 */
const RULE_GRID = 'grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-1.5';

export function ModuleBaseUrlTable({
  env,
  modules,
  mode = 'project',
  setEnvironmentModuleBaseUrl,
}: ModuleBaseUrlTableProps) {
  const single = mode === 'module';
  const topGrid = single ? TOP_GRID_SINGLE : TOP_GRID;

  /** 未提交的规则草稿，按模块 id 分桶。缺席 = 直接读落库的那一份。 */
  const [drafts, setDrafts] = useState<Record<string, RewriteRule[]>>({});
  /** 展开了规则区的模块。缺席时按「这个模块有没有规则」决定。 */
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [dragOver, setDragOver] = useState<{
    moduleId: string;
    index: number;
    zone: RewriteDropZone;
  } | null>(null);
  const [focusTarget, setFocusTarget] = useState<{ moduleId: string; index: number } | null>(null);
  const fromInputs = useRef(new Map<string, HTMLInputElement>());

  const persisted = useCallback(
    (moduleId: string): RewriteRule[] => env.moduleBaseUrls[moduleId]?.rewrites ?? NO_RULES,
    [env],
  );
  const rulesOf = (moduleId: string): RewriteRule[] => drafts[moduleId] ?? persisted(moduleId);

  // 落库的那一份追上了草稿（保存成功）时由 `commit` 自己交还草稿——不能用「相等」推断，
  // 因为 store 的乐观写入在 IPC 返回之前就把新规则放进去了：相等只能说「已经发出去了」，
  // 说不了「已经存下去了」。那一次写失败时 store 回滚，草稿若也一起丢，用户的输入就没了。

  // 新加的规则行挂载后聚焦它的起点输入框：`+ 添加规则` 之后可以直接输入。
  useEffect(() => {
    if (!focusTarget) return;
    const el = fromInputs.current.get(`${focusTarget.moduleId}:${focusTarget.index}`);
    if (!el) return;
    el.focus();
    setFocusTarget(null);
  }, [focusTarget, drafts]);

  const writeDraft = (moduleId: string, next: RewriteRule[]) =>
    setDrafts((prev) => ({ ...prev, [moduleId]: next }));

  /**
   * 把一份草稿提交到那一格。有错误时**不提交**：草稿与就地报错一起留在屏幕上，用户改到合法
   * 为止。合法的内容与落库的那份相同时也不发 IPC。
   *
   * 草稿只在写**成功**之后交还，且只在它仍是刚才提交的那一份时（用户可能已经接着改了下一处）。
   * 写失败时 store 已回滚，草稿留着，屏幕上仍是用户的输入。
   */
  const commit = async (moduleId: string, next: readonly RewriteRule[]) => {
    const persistable = cleanPersistableRewrites(next);
    if (validateRewriteRules(persistable).hasError) return;
    if (areRewriteListsEqual(persistable, persisted(moduleId))) return;
    const saved = await setEnvironmentModuleBaseUrl(env.id, moduleId, null, persistable);
    if (!saved) return;
    setDrafts((prev) => {
      const draft = prev[moduleId];
      if (!draft || !areRewriteListsEqual(draft, next)) return prev;
      const rest = { ...prev };
      delete rest[moduleId];
      return rest;
    });
  };

  const addRule = (moduleId: string) => {
    const next = [...rulesOf(moduleId), { from: '', to: '' }];
    writeDraft(moduleId, next);
    setExpanded((prev) => ({ ...prev, [moduleId]: true }));
    setFocusTarget({ moduleId, index: next.length - 1 });
  };

  const editRule = (moduleId: string, index: number, patch: Partial<RewriteRule>) => {
    const next = rulesOf(moduleId).map((rule, i) => (i === index ? { ...rule, ...patch } : rule));
    writeDraft(moduleId, next);
    return next;
  };

  const moveRule = (moduleId: string, fromIndex: number, toIndex: number) => {
    const rules = rulesOf(moduleId);
    const next = reorderRewriteRules(
      rules,
      fromIndex,
      toIndex,
      fromIndex < toIndex ? 'below' : 'above',
    );
    if (next === rules) return;
    writeDraft(moduleId, next);
    void commit(moduleId, next);
  };

  const deleteRule = (moduleId: string, index: number) => {
    const next = rulesOf(moduleId).filter((_, i) => i !== index);
    writeDraft(moduleId, next);
    void commit(moduleId, next);
  };

  const dropRule = (moduleId: string, targetIndex: number, zone: RewriteDropZone) => {
    const source = draggingRule;
    draggingRule = null;
    setDragOver(null);
    if (!source || source.moduleId !== moduleId) return;
    const rules = rulesOf(moduleId);
    const next = reorderRewriteRules(rules, source.index, targetIndex, zone);
    if (next === rules) return;
    writeDraft(moduleId, next);
    void commit(moduleId, next);
  };

  if (modules.length === 0) {
    // 行集是 (环境 × 模块)，没有模块就无从配置 —— 每个格子都无法寻址。故直接指出修法，
    // 而不是渲染一张空网格。
    return (
      <div className="px-4 py-6 text-center text-micro text-muted-foreground">
        该项目还没有模块 · 先在左侧资源树新建模块，地址才有归属
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      <div className={cn(topGrid, 'border-b border-border/60 px-4 py-2')}>
        {!single && <span className="label-caps">模块</span>}
        <span className="label-caps">模块地址</span>
        <span className="label-caps" title="按顺序匹配端点自己 URL 的路径前缀，命中首条即停止">
          路径改写
        </span>
      </div>

      {modules.map((module) => {
        const rules = rulesOf(module.id);
        const open = single || (expanded[module.id] ?? persisted(module.id).length > 0);
        const validation = validateRewriteRules(rules);
        const shadowed = findShadowedRuleIndices(rules);
        return (
          <div key={module.id} className="border-t border-border/60 first:border-t-0">
            <div className={cn(topGrid, 'px-4 py-1.5')}>
              {!single && (
                <div className="flex min-w-0 items-center gap-1.5" title={module.name}>
                  <FolderTree className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  <span className="truncate text-xs font-medium">{module.name}</span>
                </div>
              )}

              {/* 模块地址 —— 本模块在此环境下的地址。只给通用提示：在此点名另一个格子
                  等于承诺一种并不存在的继承。规则那一半送 `null` = 保持原值。 */}
              <div className="min-w-0">
                <Input
                  value={env.moduleBaseUrls[module.id]?.url ?? ''}
                  onChange={(e) =>
                    setEnvironmentModuleBaseUrl(env.id, module.id, e.target.value, null)
                  }
                  placeholder="http:// 或 https:// 起始的地址"
                  aria-label={`${module.name} · 模块地址`}
                  className="h-8 font-mono text-xs"
                  spellCheck={false}
                />
              </div>

              <div className="flex items-center gap-1">
                {!single && (
                  <Button
                    variant="ghost"
                    size="xs"
                    aria-expanded={open}
                    aria-label={`${module.name} · 展开路径改写规则`}
                    className="gap-1 text-nano text-muted-foreground"
                    onClick={() => setExpanded((prev) => ({ ...prev, [module.id]: !open }))}
                  >
                    {open ? (
                      <ChevronDown className="h-3.5 w-3.5" />
                    ) : (
                      <ChevronRight className="h-3.5 w-3.5" />
                    )}
                    {rules.length > 0 ? `${rules.length} 条` : '未配置'}
                  </Button>
                )}
                <Button
                  variant="outline"
                  size="xs"
                  className="h-7 gap-1 text-nano"
                  onClick={() => addRule(module.id)}
                >
                  <Plus className="h-3.5 w-3.5" />
                  添加改写
                </Button>
              </div>
            </div>

            {open && (
              <div className="border-t border-border/40 bg-card/20 px-4 py-1.5">
                <div className="flex items-center justify-between gap-2 pb-1">
                  <span className="text-nano text-muted-foreground">
                    路径改写规则 · 按从上到下顺序匹配，命中首条后不再向后匹配
                  </span>
                  <Button
                    variant="ghost"
                    size="xs"
                    className="h-6 gap-1 text-nano"
                    onClick={() => addRule(module.id)}
                  >
                    <Plus className="h-3 w-3" />
                    添加规则
                  </Button>
                </div>

                {rules.length === 0 ? (
                  <p className="py-2 text-micro text-muted-foreground">
                    暂无路径改写规则 · 端点自己的 URL 原样使用
                  </p>
                ) : (
                  <ol className="flex flex-col">
                    {rules.map((rule, index) => {
                      const error = validation.errorsByIndex[index];
                      const shadow = shadowed[index];
                      const showDropAbove =
                        dragOver?.moduleId === module.id &&
                        dragOver.index === index &&
                        dragOver.zone === 'above';
                      const showDropBelow =
                        dragOver?.moduleId === module.id &&
                        dragOver.index === index &&
                        dragOver.zone === 'below';
                      return (
                        <li
                          key={`${module.id}:${index}`}
                          className="relative py-1.5"
                          onDragOver={(e) => {
                            if (!draggingRule || draggingRule.moduleId !== module.id) return;
                            e.preventDefault();
                            e.dataTransfer.dropEffect = 'move';
                            const rect = e.currentTarget.getBoundingClientRect();
                            const zone = resolveRewriteDropZone(e.clientY, rect);
                            if (
                              dragOver?.moduleId === module.id &&
                              dragOver.index === index &&
                              dragOver.zone === zone
                            ) {
                              return;
                            }
                            setDragOver({ moduleId: module.id, index, zone });
                          }}
                          onDrop={(e) => {
                            e.preventDefault();
                            const rect = e.currentTarget.getBoundingClientRect();
                            dropRule(module.id, index, resolveRewriteDropZone(e.clientY, rect));
                          }}
                        >
                          {showDropAbove && (
                            <span className="pointer-events-none absolute inset-x-0 top-0 h-0.5 bg-primary" />
                          )}
                          {showDropBelow && (
                            <span className="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 bg-primary" />
                          )}

                          <div className="flex items-start gap-1.5">
                            <button
                              type="button"
                              draggable
                              aria-label={`${module.name} · 拖动第 ${index + 1} 条规则调整顺序`}
                              className="mt-1.5 cursor-grab rounded p-0.5 text-muted-foreground hover:text-foreground active:cursor-grabbing"
                              onDragStart={(e) => {
                                draggingRule = { moduleId: module.id, index };
                                e.dataTransfer.effectAllowed = 'move';
                                e.dataTransfer.setData(DRAG_MIME, String(index));
                                // 某些 WebView 只在 `text/plain` 存在时才允许拖动。
                                e.dataTransfer.setData('text/plain', rule.from);
                              }}
                              onDragEnd={() => {
                                draggingRule = null;
                                setDragOver(null);
                              }}
                            >
                              <GripVertical className="h-3.5 w-3.5" />
                            </button>

                            <span className="w-6 shrink-0 pt-2 text-nano tabular-nums text-muted-foreground">
                              #{index + 1}
                            </span>

                            <div className={cn(RULE_GRID, 'min-w-0 flex-1')}>
                              <Input
                                ref={(el) => {
                                  const key = `${module.id}:${index}`;
                                  if (el) fromInputs.current.set(key, el);
                                  else fromInputs.current.delete(key);
                                }}
                                value={rule.from}
                                onChange={(e) =>
                                  editRule(module.id, index, { from: e.target.value })
                                }
                                onBlur={() => void commit(module.id, rulesOf(module.id))}
                                onKeyDown={(e) => {
                                  if (e.key !== 'Enter' || e.nativeEvent.isComposing) return;
                                  void commit(module.id, rulesOf(module.id));
                                }}
                                placeholder="起点 /api/example"
                                aria-label={`${module.name} · 第 ${index + 1} 条改写起点`}
                                aria-invalid={!!error}
                                className={cn(
                                  'h-8 font-mono text-xs',
                                  error && 'border-destructive focus-visible:border-destructive',
                                )}
                                spellCheck={false}
                              />
                              <ArrowRight className="h-3.5 w-3.5 text-muted-foreground" />
                              <Input
                                value={rule.to}
                                onChange={(e) => editRule(module.id, index, { to: e.target.value })}
                                onBlur={() => void commit(module.id, rulesOf(module.id))}
                                onKeyDown={(e) => {
                                  if (e.key !== 'Enter' || e.nativeEvent.isComposing) return;
                                  void commit(module.id, rulesOf(module.id));
                                }}
                                placeholder="目标 /api/project-example-example"
                                aria-label={`${module.name} · 第 ${index + 1} 条改写目标`}
                                aria-invalid={!!error}
                                className={cn(
                                  'h-8 font-mono text-xs',
                                  error && 'border-destructive focus-visible:border-destructive',
                                )}
                                spellCheck={false}
                              />
                            </div>

                            <div className="flex items-center gap-0.5">
                              <Button
                                variant="ghost"
                                size="icon"
                                className="h-7 w-7"
                                title="上移"
                                aria-label={`${module.name} · 第 ${index + 1} 条规则上移`}
                                disabled={index === 0}
                                onClick={() => moveRule(module.id, index, index - 1)}
                              >
                                <ArrowUp className="h-3.5 w-3.5" />
                              </Button>
                              <Button
                                variant="ghost"
                                size="icon"
                                className="h-7 w-7"
                                title="下移"
                                aria-label={`${module.name} · 第 ${index + 1} 条规则下移`}
                                disabled={index === rules.length - 1}
                                onClick={() => moveRule(module.id, index, index + 1)}
                              >
                                <ArrowDown className="h-3.5 w-3.5" />
                              </Button>
                              <Button
                                variant="ghost"
                                size="icon"
                                className="h-7 w-7 text-muted-foreground hover:text-destructive"
                                title="删除这条规则"
                                aria-label={`${module.name} · 删除第 ${index + 1} 条规则`}
                                onClick={() => deleteRule(module.id, index)}
                              >
                                <Trash2 className="h-3.5 w-3.5" />
                              </Button>
                            </div>
                          </div>

                          {error && (
                            <InlineError className="mt-1 px-2 py-1 text-nano">{error}</InlineError>
                          )}
                          {!error && shadow !== undefined && (
                            <p className="mt-1 text-nano text-warning-foreground">
                              前置规则 #{shadow + 1}「{rules[shadow]?.from}」会先匹配这一类路径，
                              本条可能不生效 · 可拖拽上移调整顺序
                            </p>
                          )}
                        </li>
                      );
                    })}
                  </ol>
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
