//! # fluxeme-skill-backing
//!
//! Skill Runtime / Backing 数据面子系统：**部署 / 鉴权 / 路由 / 代理 / 计量**。
//!
//! 独立性：
//! - 只依赖 `fluxeme-contract`（Port 接口），**不 import** `skillhub` 与根代码。
//! - SkillHub 发布的事实经 outbox（`agent_skill_runtime_tasks`）可靠到达；
//!   需要目录信息时通过 `SkillRuntimeCatalog` Port 读取（实现方是 SkillHub）。
//! - 请求鉴权经 `ApiKeyAuthorizer` Port（根实现），计量经 `RuntimeMeter` Port（根实现）。
//!
//! 领域命名采用 **Agent Skill**（厂商无关）。

pub mod domain;
pub mod manifest;
pub mod policy;
pub mod repo;
pub mod runtime;

pub use crate::runtime::{RuntimePath, SkillBackingModule};
