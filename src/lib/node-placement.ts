import type { ResourceNodeType } from '@/types/domain';

/**
 * 资源树的父子类型矩阵 —— 全应用只在这里声明一次。
 *
 * 这张表是 `simple_post_core::domain::ResourceNodeType::may_under` 的镜像。后端在每一次
 * 写入时执行它（`db::nodes` 的应用层检查，加 `schema.sql` 里的两个触发器），前端在
 * 拖放判定与乐观更新时读它。两侧由 `fixtures/placement-parity.json` 钉住：改了一边而
 * 没改另一边，那条样本会红。
 *
 * `project` 是「项目顶层」这个作用域 —— 模块的父级是项目而不是节点，故它也要占一行。
 */
export type PlacementScope = ResourceNodeType | 'project';

/** 每一个作用域接受哪几种子类型。 */
export const ACCEPTED_CHILDREN: Record<PlacementScope, readonly ResourceNodeType[]> = {
  // 项目顶层只有模块。集合与接口的父级永远是节点，落不到项目顶层。
  project: ['module'],
  // 模块收集合与接口 —— 接口不需要自己的集合时可以直接挂在模块下。
  module: ['collection', 'endpoint'],
  // 集合收接口与更多集合，嵌套深度无上限。
  collection: ['collection', 'endpoint'],
  // 接口是叶子，不收任何子节点。
  endpoint: [],
};

/**
 * `child` 能不能直接挂在 `parent` 这个作用域下。
 *
 * `parent` 传节点类型即「挂到那个类型的节点下」，传 `'project'` 即「落到项目顶层」。
 */
export function mayUnder(child: ResourceNodeType, parent: PlacementScope): boolean {
  return ACCEPTED_CHILDREN[parent].includes(child);
}
