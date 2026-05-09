# OpenVault 热规则 (Hot Rules)

> 这是项目的核心行为准则，当规则与实现冲突时，以规则为准。

## Phase 1-3 核心规则

### 1. 永不丢失 (Never Lose Data)
- **底线规则**，任何设计决策优先保证数据安全
- 备份必须可验证、可恢复
- 删除前必须确认存在至少一份有效副本

### 2. 自愈优先于告警 (Self-Healing > Alerting)
- 能自动修复的不通知人
- 自动校验、自动修复、自动补偿
- 告警仅作为最后的兜底手段

### 3. 加密是默认行为 (Encryption by Default)
- 私密文件端到端加密
- 默认使用 AES-256-GCM
- 密钥管理独立于数据存储

### 4. 核心备份不依赖OpenLink (Core Backup Independence)
- OpenLink 是增强，不是必需
- 核心备份功能必须独立工作
- OpenLink 用于运输和远程管理

## Phase 4 新增规则

### 5. 编译环境规则
- **Rust版本**：必须用 `~/.cargo/bin/cargo` (1.95.0)
- **不要用** `/usr/bin/cargo` (1.75.0)，版本过旧
- **编译输出**：`CARGO_TARGET_DIR=/tmp/openvault-target`
- **编译并发**：`CARGO_BUILD_JOBS=1` (内存限制，3.8G经常不够)
- **卡住3次换方案**：同一crate编译失败3次→换crate，纯Rust优先

### 6. 传输路由规则
- 直传优先于云中转
- 同区域优先使用直接传输
- 网络质量好时优先直传

### 7. API安全规则
- JWT认证是必须的
- Webhook通知需要HMAC签名验证
- Token过期时间24小时，刷新token 30天

### 8. 设备管理规则
- 设备必须注册才能使用
- 心跳间隔建议5分钟
- 超过15分钟无心跳视为离线

## 更新日志
- 2024-05-10: 添加 Phase 4 热规则
