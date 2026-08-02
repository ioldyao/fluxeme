# Fluxeme 前端解耦 — 进行中

这是前端 user/admin 独立部署改造的**过渡目录**，与仓库根目录现有的 `ui/`（单体前端，仍是当前生产使用的版本）并行存在，尚未替换它。

## 当前进度

- [x] pnpm workspace 骨架搭建（`packages/shared` `packages/user-app` `packages/admin-app`）
- [x] `shared` 包迁移完成：`api/`、`types/`、`i18n/`、`store/`、`permissions/`、`lib/`、`constants/`、`components/ui/`、通用业务组件（`ConfirmDialog`、`CopyButton`、`EmptyState`、`PageHeader`、`ModelDetailDialog`、`ModelHealthCheckDialog`、`UsageLogDetail`、`DashboardChartTooltip`）
- [x] `shared` 包新增 `components/layout/`（`LayoutShell`、`SidebarPrimitives`、`TopBar`）与 `routes/`（`ProtectedRoute`、`PermissionRoute`、`SessionBootstrapper`），供两个 app 复用
- [x] `api/client.ts` 改造：支持 `VITE_API_BASE_URL` 环境变量 + `credentials: 'include'`，适配跨端口/跨域访问后端
- [x] `TopBar` 跨端跳转改造：原本的同 SPA 内部路由跳转，改为读取 `VITE_ADMIN_APP_URL`（或对称的 user 端变量）的绝对 URL + `window.location.href`，因为两个 app 不再共享同一个 router 实例
- [x] **`user-app` 业务层完成**：路由配置（`publicRoutes` + `authRoutes`，已去除 `adminRoutes`/`perm` 字段）、路由树、`UserSidebar`、`UserLayout`、9 个用户页面（Dashboard/ApiKeys/Usage/Wallet/Bills/Profile/Settings/ModelsMarketplace/MyRules）+ 3 个公共页面（Login/Register/SsoCallback）+ NotFound、`ApiKeyForm`、`index.css`、静态资源（字体/图标）
- [x] `user-app`：`tsc --noEmit` 通过；dev server 冒烟测试，`App.tsx`/路由树/关键页面均正确编译（HTTP 200）
- [x] **`admin-app` 业务层完成**：路由配置（`publicRoutes` + `adminRoutes`，`/` 重定向到 `/flow-control`）、路由树、`AdminSidebar`、`AdminLayout`、`AdminRoute`（admin-app 专属，不放 shared）、13 个管理页面（Users/Channels/Models/Rules/Moderation/FlowControlTower/FlowTowerContent/RoutingFlow/RoutingHistory/ModelPricing + admin/AdminSettings/Announcements/RechargeKeys）+ 3 个公共页面 + NotFound、4 个表单（ChannelForm/ModelForm/RuleForm/UserForm）
- [x] `shared/api/client.ts` 新增 `getWsUrl(path)`：`FlowTowerContent`/`RoutingFlow` 原本硬编码 `window.location.host` 的 WebSocket 连接（`/api/health/ws`）改用此函数，避免独立部署后连到前端自己的端口
- [x] `admin-app`：`tsc --noEmit` 通过；dev server 冒烟测试；**验证两个 dev server（5173 + 5174）可同时运行、互不干扰**
- [ ] 两个 app 互相配置 `VITE_ADMIN_APP_URL`/`VITE_USER_APP_URL` 实际值
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
