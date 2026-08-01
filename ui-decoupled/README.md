# Fluxeme 前端解耦 — 进行中

这是前端 user/admin 独立部署改造的**过渡目录**，与仓库根目录现有的 `ui/`（单体前端，仍是当前生产使用的版本）并行存在，尚未替换它。

## 当前进度

- [x] pnpm workspace 骨架搭建（`packages/shared` `packages/user-app` `packages/admin-app`）
- [x] `shared` 包迁移完成：`api/`、`types/`、`i18n/`、`store/`、`permissions/`、`lib/`、`components/ui/`、通用业务组件（`ConfirmDialog`、`CopyButton`、`EmptyState`、`PageHeader`、`ModelDetailDialog`、`ModelHealthCheckDialog`、`UsageLogDetail`、`DashboardChartTooltip`）
- [x] `api/client.ts` 改造：支持 `VITE_API_BASE_URL` 环境变量 + `credentials: 'include'`，适配跨端口/跨域访问后端
- [x] 两个 app 的 `tsc --noEmit` 类型检查通过，dev server 冒烟测试验证 `@shared/*` 别名运行时可用
- [ ] `user-app` 业务页面迁移（Dashboard, ApiKeys, Usage, Wallet, Bills, Profile, Settings, ModelsMarketplace, MyRules）+ UserLayout/Sidebar/路由
- [ ] `admin-app` 业务页面迁移（Users, Channels, Models, Rules, Moderation, FlowControlTower 等）+ AdminLayout/Sidebar/路由
- [ ] `index.css`（Tailwind 主题变量）迁移
- [ ] 后端 CORS 配置调整，支持两个前端跨端口访问
- [ ] 验证通过后，替换根目录 `ui/`，更新 Rust 后端的静态资源托管路径

## 本地跑法（当前仅骨架，非完整功能）

```bash
cd ui-decoupled
pnpm install
pnpm dev:user   # http://localhost:5173
pnpm dev:admin  # http://localhost:5174
```

后端仍按原方式在 `:8080` 启动。

## 目录结构

```
ui-decoupled/
├── pnpm-workspace.yaml
├── package.json
└── packages/
    ├── shared/       # 共享代码：api client、UI 组件、types、i18n、store、permissions
    ├── user-app/      # 独立 Vite 项目，端口 5173
    └── admin-app/     # 独立 Vite 项目，端口 5174
```
