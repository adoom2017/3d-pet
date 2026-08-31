# DesktopPet 阶段任务与验收门禁

本文是 DesktopPet MVP 的唯一阶段执行清单。产品范围见 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)，实现边界见 [ARCHITECTURE.md](ARCHITECTURE.md)。阶段必须按 Phase 0 到 Phase 14 串行执行，前一阶段未达到 `Done` 时禁止开始下一阶段。

## 1. 状态与工作流

允许的状态只有：

- `Blocked`：前置阶段或外部条件未满足，禁止实现。
- `Ready`：前置阶段全部完成，可以成为唯一活动阶段。
- `In Progress`：正在实现；全项目最多一个 Phase 处于此状态。
- `Verifying`：实现已停止变更，正在跑自动和人工验收；下一阶段仍保持 `Blocked`。
- `Done`：退出条件全部满足，证据完整，允许把下一阶段改为 `Ready`。

状态只允许按 `Blocked -> Ready -> In Progress -> Verifying -> Done` 前进。验证失败时退回 `In Progress`。不得跳过状态，不得因为“代码已写完”直接标记 `Done`。

### 初始状态

| Phase | 名称 | 状态 |
| --- | --- | --- |
| 0 | 工程基线 | Done |
| 1 | 透明窗口 | Done |
| 2 | wgpu 基线 | Done |
| 3 | 静态 GLB | Done |
| 4 | 骨骼与 Idle | In Progress |
| 5 | Idle / Walk Cross Fade | Blocked |
| 6 | 桌面移动 | Blocked |
| 7 | PetBrain | Blocked |
| 8 | 边界与显示器 | Blocked |
| 9 | 命中测试 | Blocked |
| 10 | 平台鼠标穿透 | Blocked |
| 11 | 拖动 | Blocked |
| 12 | 重力与落地 | Blocked |
| 13 | Look At Mouse | Blocked |
| 14 | 性能与 MVP 验收 | Blocked |

## 2. 全局硬性门禁

每个 Phase 在进入 `Done` 前必须执行并通过：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

还必须满足：

- GitHub Actions 在每次 `push` 和 `pull_request` 运行 macOS / Windows 矩阵；候选 commit 的矩阵全绿，且每个 job 执行上述四条命令。
- 涉及视觉或交互的阶段完成 macOS 人工验收。
- Windows 只要求 CI 中的编译、Clippy、测试和构建通过，不设置 Windows 实机门禁；未取得证据时不得声称 Windows 运行时、视觉或交互行为已验证。
- 新逻辑有确定性自动测试；测试不能依赖真实时间、非固定随机源或执行顺序。
- 没有跳过、忽略或临时注释失败测试；没有未说明的 warning。
- 验收记录包含绝对日期、完整 commit SHA、平台与系统版本、硬件/adapter（相关时）、执行结果以及截图或日志路径。

推荐证据目录：

```text
evidence/
├── phase-00/
│   └── verification.md
├── phase-01/
│   ├── verification.md
│   └── screenshots/
└── ...
```

每份 `verification.md` 至少使用以下记录格式：

```markdown
| 日期 | Commit | 平台 | 验收项 | 结果 | 证据路径 |
| --- | --- | --- | --- | --- | --- |
| YYYY-MM-DD | <full-sha> | macOS <version> / Windows <version> | <item> | Pass / Fail | <path> |
```

截图必须能识别被验收行为；命令日志必须保留命令、退出码和关键输出。失败证据不删除，修复后追加新的通过记录。若 Phase 改为 `Done` 后代码变更影响其契约，应重新验证受影响阶段，必要时将状态退回 `Verifying`。

## 3. Phase 0：工程基线

**状态：`Done`**

**目标：** 建立可在 macOS 开发、在 macOS / Windows CI 编译测试的单应用 Cargo workspace，具备入口、错误、日志、测试和依赖兼容性基线。

**非目标：** 不创建透明窗口，不初始化 wgpu，不加入宠物资产或业务功能。

**前置条件：** Rust stable toolchain 可用；已确认项目目录和远程仓库归属。无前置 Phase。

**实现任务：**

- [x] 初始化 Git 仓库（若尚不存在），配置适合 Rust 和平台构建产物的 `.gitignore`。
- [x] 创建 resolver 2 的 Cargo workspace 和单一 `crates/desktop-pet` 应用 crate。
- [x] 建立 `main`、`app`、`config`、`error`、`time` 以及架构中的模块骨架，保持未实现模块最小化。
- [x] 选择相互兼容的稳定 winit / wgpu 与基础依赖，生成并保留 `Cargo.lock`。
- [x] 在 `docs/dependency-compatibility.md` 记录选择日期、Rust 版本、关键 crate 版本、兼容性依据和已知平台限制。
- [x] 接入 `tracing` 启动/退出日志、模块错误类型和应用边界错误上下文；生产路径无无理由的 `unwrap`。
- [x] 建立单元测试、integration test 与 `tests/fixtures` 结构，并加入最小启动/配置测试。
- [x] 创建由 `push` 和 `pull_request` 触发的 GitHub Actions `macos-latest` / `windows-latest` 矩阵，执行四条全局门禁命令。
- [x] 创建 README，记录开发前置、构建运行、测试命令和三份设计文档入口。

**自动验证命令：** 执行四条全局门禁命令；另执行 `cargo run -p desktop-pet`，确认进程正常启动并可干净退出。

**macOS 人工验收：** 在 macOS 启动应用，确认有版本/平台启动日志、无 panic，并能通过正常退出路径结束。

**Windows CI 验收：** Windows job 安装稳定 Rust 后完成格式、Clippy、测试和 debug 构建；不得用 `continue-on-error` 掩盖失败。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** `Cargo.toml`、应用 crate、`Cargo.lock`、`.github/workflows/ci.yml`、README、依赖兼容性记录，以及 `evidence/phase-00/verification.md` 中的本地日志和双平台 CI run URL。

**阶段退出条件：** 所有实现任务完成，四条命令和 macOS 启动通过，CI 双平台全绿，证据记录完整。

**下一阶段解锁条件：** Phase 0 标记 `Done` 后，Phase 1 才能从 `Blocked` 改为 `Ready`。

## 4. Phase 1：透明窗口

**状态：`Done`**

**目标：** 创建 320 x 320 逻辑像素、无边框、透明、不可缩放、置顶的窗口，并验证系统合成与窗口层级。

**非目标：** 不初始化 wgpu，不绘制三角形，不实现鼠标穿透或宠物命中。

**前置条件：** Phase 0 为 `Done`。

**实现任务：**

- [x] 在主线程建立 winit event loop 和版本对应的应用生命周期。
- [x] 配置 320 x 320 逻辑尺寸、decorations false、transparent true、resizable false 和 always-on-top。
- [x] 在 `PlatformBackend` 两端实现或补强置顶能力，平台条件编译只存在于 `platform/**`。
- [x] 正确处理 close、resize、scale-factor 和零尺寸事件，正常关闭时有结构化日志。
- [x] 为窗口配置映射和非平台逻辑添加自动测试。

**自动验证命令：** 执行四条全局门禁命令；运行窗口配置单元测试和 `cargo run -p desktop-pet` 启动 smoke。

**macOS 人工验收：** 截图或录屏证明窗口背景真实透明、没有边框和阴影、尺寸符合预期、不可缩放，并在普通应用窗口之上；确认关闭无崩溃。

**Windows CI 验收：** Windows job 完成全部门禁，含 Windows 平台模块编译和测试。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** 窗口生命周期代码、平台置顶实现、相关测试，`evidence/phase-01/verification.md`、macOS 截图或录屏和 CI URL。

**阶段退出条件：** macOS 实机行为通过，自动门禁和 macOS / Windows CI 全绿，视觉证据完整。

**下一阶段解锁条件：** Phase 1 标记 `Done` 后，Phase 2 才能改为 `Ready`。

## 5. Phase 2：wgpu 基线

**状态：`Done`**

**目标：** 初始化 adapter、device、queue 和透明 surface，以 alpha 0 清屏并绘制可见三角形。

**非目标：** 不加载 GLB，不实现材质、骨骼或动画。

**前置条件：** Phase 1 为 `Done`，透明窗口已在 macOS 实机通过。

**实现任务：**

- [x] 建立 `Renderer`，拥有 instance、adapter、device、queue、surface 和 pipeline。
- [x] 选择受支持的 surface format、present mode 和透明 alpha mode，并记录 adapter 信息。
- [x] 使用 `(0, 0, 0, 0)` 清屏，创建最小 WGSL shader 和三角形 pipeline。
- [x] 实现非零尺寸 resize、scale-factor change、surface lost/outdated 恢复、timeout 和 out-of-memory 分类。
- [x] 增加 adapter/device smoke test 与离屏三角形像素断言；无 adapter 时只允许明确的测试环境 skip 策略。

**自动验证命令：** 执行四条全局门禁命令；运行 renderer adapter 和离屏渲染 smoke tests。

**macOS 人工验收：** 确认三角形颜色内容可见、三角形以外桌面真实透明、resize/DPI 事件无黑底或崩溃。

**Windows CI 验收：** Windows job 编译 D3D backend 路径并执行可用的 renderer smoke；任何环境 skip 必须在日志中可见。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** renderer、shader、smoke tests，`evidence/phase-02/verification.md`、macOS 截图、adapter 日志和 CI URL。

**阶段退出条件：** macOS 透明 surface 和可见 GPU 内容实机通过，错误恢复测试与全局门禁通过。

**下一阶段解锁条件：** Phase 2 标记 `Done` 后，Phase 3 才能改为 `Ready`。

## 6. Phase 3：静态 GLB

**状态：`Done`**

**目标：** 通过受校验的 manifest 加载 Quaternius CC0 GLB，并渲染静态网格、纹理和材质。

**非目标：** 不实现 skin、骨骼采样、Idle 或 Walk。

**前置条件：** Phase 2 为 `Done`；已选定满足 CC0、GLB、rigged、Idle/Walk 条件的 Quaternius Animated Animals 资产。

**实现任务：**

- [x] 按架构定义实现 `AssetManager`、manifest schema、路径约束和大小/数量上限。
- [x] 保存来源 URL、作者、CC0 许可证、获取日期、SHA-256 和实际动画名称。
- [x] 解析 GLB scene、node transform、mesh primitive、index/vertex、纹理、sampler 和 MVP 所需 PBR material 属性。
- [x] 将 CPU 资源与 GPU 上传分离，Renderer 不直接读文件。
- [x] 实现 camera、深度缓冲和模型 transform，使模型在 320 x 320 viewport 内完整可见且比例稳定。
- [x] 添加有效、缺失、路径逃逸、损坏、超限和错误动画映射 fixture 测试。

**自动验证命令：** 执行四条全局门禁命令；运行 asset fixture tests、GLB parse tests 和静态模型离屏 smoke。

**macOS 人工验收：** 确认默认宠物方向、比例、材质、纹理、深度和透明边缘正确，窗口内不裁掉关键部位。

**Windows CI 验收：** Windows job 完成资源解析、fixture、离屏测试和构建；验证路径规则在 Windows separator 下成立。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** manifest、许可证与来源记录、默认 GLB、AssetManager、静态 renderer、fixtures，以及 `evidence/phase-03/verification.md`、macOS 截图和 CI URL。

**阶段退出条件：** 默认资产可信记录完整，静态模型在 macOS 正确显示，资源失败路径有测试，所有门禁通过。

**下一阶段解锁条件：** Phase 3 标记 `Done` 后，Phase 4 才能改为 `Ready`。

## 7. Phase 4：骨骼与 Idle

**状态：`In Progress`**

**目标：** 实现 skin、joint matrix、动画 channel 采样并稳定循环 Idle。

**非目标：** 不播放 Walk，不做 Cross Fade 或程序化头部叠加。

**前置条件：** Phase 3 为 `Done`；默认资源 Idle 语义映射已确认。

**实现任务：**

- [ ] 加载 skeleton hierarchy、joint、inverse bind matrix 和 bind pose，校验索引及 GPU joint limit。
- [ ] 解析 translation、rotation、scale channel 与 step/linear 插值；明确不支持项并返回错误。
- [ ] 计算 local pose、global pose 和最终 joint matrices，上传 shader skinning buffer。
- [ ] 实现 Idle clip 时间推进、循环边界和固定 dt 采样。
- [ ] 添加 bind pose、单 joint、层级 joint、循环首尾、缺失 channel 和畸形 skin 的确定性测试。

**自动验证命令：** 执行四条全局门禁命令；运行 skeleton、animation sampling 和 skinning 离屏 tests。

**macOS 人工验收：** 连续观察多个 Idle 周期，确认骨骼姿态正确、无爆炸/塌陷、循环接缝无明显跳变。

**Windows CI 验收：** Windows job 通过骨骼数学、采样、buffer layout 和构建测试。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** skeleton/animation 数据结构、skinning shader、测试，以及 `evidence/phase-04/verification.md`、macOS 视频或连续截图和 CI URL。

**阶段退出条件：** Idle 在 macOS 实机无明显变形或循环错误，全部数学测试与全局门禁通过。

**下一阶段解锁条件：** Phase 4 标记 `Done` 后，Phase 5 才能改为 `Ready`。

## 8. Phase 5：Idle / Walk Cross Fade

**状态：`Blocked`**

**目标：** 通过语义动画名称播放 Idle / Walk，支持循环、速度倍率和 250 ms Cross Fade。

**非目标：** 不移动窗口，不加入随机行为。

**前置条件：** Phase 4 为 `Done`；默认资源 Walk 映射已确认。

**实现任务：**

- [ ] 实现 `AnimationController` 和 `AnimationRequest`，业务层只使用 `idle` / `walk` 语义。
- [ ] 实现 clip 循环、播放速度、重复请求幂等和不存在 clip 的可读错误。
- [ ] 对两侧 local joint transform 做 250 ms Cross Fade，再计算全局 joint matrices。
- [ ] 覆盖 Idle -> Walk、Walk -> Idle、过渡反向、过渡中重新请求和首尾循环。
- [ ] 用固定输入断言 0 ms、125 ms、250 ms 姿态，避免只靠视觉判断。

**自动验证命令：** 执行四条全局门禁命令；运行 AnimationController 确定性 tests 和 skinning smoke。

**macOS 人工验收：** 反复触发 Idle / Walk 双向切换，确认无明显 pop、关节穿插突变或 clip 重启抖动。

**Windows CI 验收：** Windows job 通过全部动画过渡数值测试和构建。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** AnimationController、过渡测试，`evidence/phase-05/verification.md`、macOS 录屏和 CI URL。

**阶段退出条件：** 250 ms 双向过渡数值测试与 macOS 视觉验收通过，所有门禁通过。

**下一阶段解锁条件：** Phase 5 标记 `Done` 后，Phase 6 才能改为 `Ready`。

## 9. Phase 6：桌面移动

**状态：`Blocked`**

**目标：** 以桌面逻辑坐标驱动窗口水平移动，同时播放 Walk，行为速度不依赖渲染 FPS。

**非目标：** 不做随机决策、多显示器夹紧或边缘转身。

**前置条件：** Phase 5 为 `Done`；PlatformBackend 已能读写窗口位置。

**实现任务：**

- [ ] 引入 `DesktopPosition` 和 `PhysicsBody`，将窗口桌面位置与模型局部/world transform 分离。
- [ ] 以逻辑像素/秒和固定 dt 积分水平速度，集中处理系统位置舍入。
- [ ] `Walking` 状态同步 Walk 请求、朝向和窗口位置；停止时同步 Idle。
- [ ] 平台移动失败时保留上一次确认位置并输出带上下文错误。
- [ ] 测试 15/30/60/120 render FPS 下相同模拟时间得到相同逻辑位置。

**自动验证命令：** 执行四条全局门禁命令；运行 movement fixed-dt、rounding 和平台 mock tests。

**macOS 人工验收：** 让宠物在单屏内从左向右移动，确认动画、朝向、窗口位置一致，无明显速度漂移或抖动。

**Windows CI 验收：** Windows job 编译窗口位置平台实现并通过 mock/纯逻辑测试。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** movement/physics 基础、平台位置方法、测试，`evidence/phase-06/verification.md`、macOS 录屏和 CI URL。

**阶段退出条件：** 移动距离对 FPS 不敏感，macOS 动画与窗口同步，全部门禁通过。

**下一阶段解锁条件：** Phase 6 标记 `Done` 后，Phase 7 才能改为 `Ready`。

## 10. Phase 7：PetBrain

**状态：`Blocked`**

**目标：** 用可注入随机源和模拟时钟实现可复现的 Idle、Walk、Turn 自主行为。

**非目标：** 不加入 AI、鼠标驱动决策、边界检测或 Sleep 随机行为。

**前置条件：** Phase 6 为 `Done`；状态机能执行 Idle / Walking / Turning 意图。

**实现任务：**

- [ ] 定义 `PetObservation`、`PetIntent`、`RandomSource`、时钟边界和 `PetBrain`。
- [ ] 配置 Idle 等待范围、Walk 持续时间、方向选择和 Turn 规则，参数集中且有合法范围。
- [ ] Brain 只输出意图，不调用窗口、动画、资源或 renderer。
- [ ] 状态机拒绝非法转换，高优先级状态可抑制普通 Brain 意图。
- [ ] 使用固定 seed 和模拟时钟断言完整决策序列、边界概率和重放一致性。

**自动验证命令：** 执行四条全局门禁命令；运行 brain deterministic、state-machine transition 和 invalid-intent tests。

**macOS 人工验收：** 观察多个自主周期，确认 Idle / Walk / Turn 自然衔接、无状态卡死；录制日志中的 intent/state 序列。

**Windows CI 验收：** Windows job 运行同一 seed 并得到相同决策序列。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** Brain、状态机、可注入随机/时钟和测试，`evidence/phase-07/verification.md`、观察日志和 CI URL。

**阶段退出条件：** 固定 seed 完全可复现，业务依赖边界符合架构，macOS 自主行为和全部门禁通过。

**下一阶段解锁条件：** Phase 7 标记 `Done` 后，Phase 8 才能改为 `Ready`。

## 11. Phase 8：边界与显示器

**状态：`Blocked`**

**目标：** 支持显示器工作区、负坐标、不同 DPI、窗口跨屏、边缘夹紧和转身。

**非目标：** 不感知其他应用窗口，不实现命中或鼠标穿透。

**前置条件：** Phase 7 为 `Done`；PlatformBackend 可返回归一化显示器信息。

**实现任务：**

- [ ] 实现 `MonitorInfo` 快照和 `DisplayManager`，统一桌面左上角逻辑坐标。
- [ ] 使用工作区而非完整屏幕尺寸计算左右边缘和地面。
- [ ] 实现窗口中心优先、最大相交面积其次、primary 最后的活动显示器规则。
- [ ] 支持负 desktop origin、不同 scale factor、显示器热插拔和空列表降级。
- [ ] 在越界前夹紧位置并产生 Turn，避免逐帧撞边反复翻转。
- [ ] 覆盖单屏、左侧负坐标屏、上下排列、跨屏、125%/200% 和 Retina 等表驱动测试。

**自动验证命令：** 执行四条全局门禁命令；运行 display conversion、monitor selection、clamp 和 boundary-turn tests。

**macOS 人工验收：** 在可用的单屏/多屏及 Retina 场景移动窗口，确认不进入 Dock 工作区、不消失、跨屏不跳变，证据记录实际布局。

**Windows CI 验收：** Windows job 通过负坐标和多 DPI 纯逻辑测试，并编译显示器枚举实现。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** DisplayManager、显示器平台适配、边界逻辑和测试，`evidence/phase-08/verification.md`、布局示意/录屏和 CI URL。

**阶段退出条件：** 全部坐标测试通过，macOS 可用布局无越界或跳变，所有门禁通过。

**下一阶段解锁条件：** Phase 8 标记 `Done` 后，Phase 9 才能改为 `Ready`。

## 12. Phase 9：命中测试

**状态：`Blocked`**

**目标：** 统一鼠标坐标转换，用 2D bounding region 或 hit mask 区分宠物区域与透明区域。

**非目标：** 不做 triangle raycast、body-part 命中或平台鼠标穿透。

**前置条件：** Phase 8 为 `Done`；模型在窗口中的投影区域可稳定确定。

**实现任务：**

- [ ] 实现桌面逻辑 -> 窗口逻辑 -> 物理像素 -> NDC -> 可选相机射线转换链。
- [ ] MVP 命中只使用窗口局部逻辑坐标，定义边界包含规则和透明区域行为。
- [ ] region 随宠物 scale、朝向、viewport 和 DPI 正确更新，不读取 GPU framebuffer。
- [ ] `MouseState` 同时保存可选桌面坐标与窗口局部坐标，窗口外/零尺寸返回 miss。
- [ ] 为角点、边界、负坐标、不同 DPI、零尺寸、NaN 防护和模型翻转添加测试。

**自动验证命令：** 执行四条全局门禁命令；运行 coordinate pipeline 和 hit-region table tests。

**macOS 人工验收：** 通过开发 overlay 或结构化日志沿宠物轮廓移动鼠标，确认宠物区域为 hit、可见模型以外为 miss，跨 DPI 后仍对齐。

**Windows CI 验收：** Windows job 通过相同坐标 fixture 和全部构建测试。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** MouseState、坐标转换、HitRegion、测试，`evidence/phase-09/verification.md`、overlay 截图/命中日志和 CI URL。

**阶段退出条件：** 数学测试和 macOS 轮廓验收通过，无 DPI 偏移，全部门禁通过。

**下一阶段解锁条件：** Phase 9 标记 `Done` 后，Phase 10 才能改为 `Ready`。

## 13. Phase 10：平台鼠标穿透

**状态：`Blocked`**

**目标：** 让透明区域把鼠标交给下层桌面/应用，同时宠物区域继续接收点击。

**非目标：** 不实现拖动，不升级精确 3D 命中。

**前置条件：** Phase 9 为 `Done`；命中结果在可用 DPI 场景稳定。

**实现任务：**

- [ ] 在 `PlatformBackend` 实现幂等 `set_click_through`，状态变化才调用原生 API。
- [ ] Windows 在 `platform/windows/**` 处理 `WM_NCHITTEST`，hit 返回 `HTCLIENT`、miss 返回 `HTTRANSPARENT`。
- [ ] macOS 在 `platform/macos/**` 动态控制 `ignoresMouseEvents`，忽略期间仍轮询/接收全局光标唤醒以恢复交互。
- [ ] 将宠物区域的完整 click 转换为 `PetIntent::Interact`；即使资源没有专用互动 clip，也必须保留可观察、可测试的状态转换。
- [ ] 处理光标高速跨越、窗口移动、focus loss、DPI change 和退出，确保不会遗留不可交互窗口。
- [ ] 为平台状态决策、幂等调用和命中映射加入 mock/平台单元测试；记录 unsafe 不变量。

**自动验证命令：** 执行四条全局门禁命令；运行 interaction-to-platform、idempotency 和 target-specific platform tests。

**macOS 人工验收：** 在透明区域点击下层桌面图标/应用并确认其收到点击；随后不重启应用，把鼠标移到宠物上确认宠物点击仍触发；覆盖快速移入移出和跨 DPI。

**Windows CI 验收：** Windows job 编译消息处理代码，通过返回值映射和平台 mock 测试。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** 两端平台穿透实现、测试，`evidence/phase-10/verification.md`、macOS 录屏/点击日志和 CI URL。

**阶段退出条件：** macOS 实机同时满足“桌面可点击”和“宠物可交互”，无永久错误状态，所有门禁通过。

**下一阶段解锁条件：** Phase 10 标记 `Done` 后，Phase 11 才能改为 `Ready`。

## 14. Phase 11：拖动

**状态：`Blocked`**

**目标：** 实现命中后按下、记录 offset、移动、释放和高优先级 `Dragged` 状态。

**非目标：** 不实现重力落地；释放后只产生已定义的 release action/velocity。

**前置条件：** Phase 10 为 `Done`；宠物区域可稳定接收 pointer down/up/move。

**实现任务：**

- [ ] 实现 Pressed / Dragged 交互状态、移动阈值、按下 offset 和 pointer capture/cancel。
- [ ] 用绝对桌面位置减 offset 计算窗口位置，不累加相邻 move delta。
- [ ] `Dragged` 抑制 Brain、普通物理和 click-through，释放后恢复正确优先级。
- [ ] 保存有界、带时间戳的移动样本并计算稳定 release velocity。
- [ ] 处理窗口边缘、跨显示器/DPI、focus loss、系统 cancel 和应用退出。
- [ ] 添加 offset、阈值、丢事件、cancel、跨 DPI 和速度计算的纯逻辑测试。

**自动验证命令：** 执行四条全局门禁命令；运行 InteractionController drag/cancel/release tests。

**macOS 人工验收：** 从宠物不同部位拖动，确认抓取点不跳；覆盖窗口边缘、快速移动和可用的跨 DPI 显示器，释放后桌面点击恢复。

**Windows CI 验收：** Windows job 通过全部交互纯逻辑和平台编译测试。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** InteractionController、pointer capture/取消路径、测试，`evidence/phase-11/verification.md`、macOS 录屏和 CI URL。

**阶段退出条件：** 拖动不跳位、不遗留 capture/click-through 状态，macOS 场景和全部门禁通过。

**下一阶段解锁条件：** Phase 11 标记 `Done` 后，Phase 12 才能改为 `Ready`。

## 15. Phase 12：重力与落地

**状态：`Blocked`**

**目标：** 拖动释放后完成 Falling -> Landing -> Idle，包含释放速度、重力积分和工作区地面夹紧。

**非目标：** 不实现弹跳、旋转刚体、窗口碰撞或完整物理引擎。

**前置条件：** Phase 11 为 `Done`；释放动作能提供桌面位置和速度。

**实现任务：**

- [ ] 在固定 dt 下应用 release velocity、重力和位置积分，单位统一为逻辑像素/秒。
- [ ] 使用活动显示器工作区计算地面；穿越地面时夹紧位置、清零垂直速度并 grounded。
- [ ] 实现 Dragged -> Falling -> Landing -> Idle 的合法转换和动画降级策略。
- [ ] 明确向上抛、向下释放、地面以下释放、超大真实时间间隔和显示器切换行为。
- [ ] 使用多个 dt 序列验证近似一致的落地时间/位置，并测试无穿透和无无限 Falling。

**自动验证命令：** 执行四条全局门禁命令；运行 physics fixed-step、ground collision 和 state transition tests。

**macOS 人工验收：** 在不同高度和速度释放，确认轨迹连续、地面位置正确、只落地一次并回到 Idle；在可用多屏场景重复。

**Windows CI 验收：** Windows job 通过相同物理 fixture 和状态转换测试。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** PhysicsBody 更新、落地状态转换和测试，`evidence/phase-12/verification.md`、macOS 录屏/状态日志和 CI URL。

**阶段退出条件：** 所有 dt 与边界测试通过，macOS 无穿地/重复落地/卡死，全部门禁通过。

**下一阶段解锁条件：** Phase 12 标记 `Done` 后，Phase 13 才能改为 `Ready`。

## 16. Phase 13：Look At Mouse

**状态：`Blocked`**

**目标：** 在基础动画后向 head joint 叠加受限、平滑、与 FPS 无关的 yaw / pitch。

**非目标：** 不实现眼球、耳朵、尾巴程序化动画或 body-part 命中。

**前置条件：** Phase 12 为 `Done`；manifest 可选 head joint 映射已接入。

**实现任务：**

- [ ] 从宠物头部和鼠标桌面位置构造 LookTarget，转换失败时禁用本帧叠加。
- [ ] 在基础 clip/Cross Fade 后应用 yaw `[-40, 40]`、pitch `[-20, 25]` 度限制。
- [ ] 用固定 dt 的指数平滑或等价 seek-safe 方法逼近目标，避免 overshoot 和 FPS 依赖。
- [ ] 保持骨骼局部轴约定可配置，避免硬编码模型特定轴散落在动画逻辑中。
- [ ] head joint 缺失时每次资源加载只 warning 一次并保持基础动画可用。
- [ ] 测试角度 clamp、中心点、四象限、平滑收敛、目标丢失和缺失 joint。

**自动验证命令：** 执行四条全局门禁命令；运行 look-at math、smoothing 和 pose-overlay tests。

**macOS 人工验收：** 沿窗口四周和平滑/快速轨迹移动鼠标，确认头部自然跟随、不翻转、不抖动，Idle 与 Walk 期间都有效。

**Windows CI 验收：** Windows job 通过相同姿态数值测试和构建。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** LookTarget、程序化 pose layer、manifest joint 配置和测试，`evidence/phase-13/verification.md`、macOS 录屏和 CI URL。

**阶段退出条件：** 数值限制和收敛测试通过，macOS 视觉无异常，缺失 joint 能安全降级，全部门禁通过。

**下一阶段解锁条件：** Phase 13 标记 `Done` 后，Phase 14 才能改为 `Ready`。

## 17. Phase 14：性能与 MVP 验收

**状态：`Blocked`**

**目标：** 实现自适应帧调度和完全静止时按事件渲染，完成 macOS 性能、长时间稳定性和完整 MVP 验收，并验证 Windows CI 兼容性。

**非目标：** 不增加托盘、设置、自动启动、签名、公证、AI 或任何 MVP 外功能。

**前置条件：** Phase 13 为 `Done`；Phase 0-13 的证据均完整且未被后续变更失效。

**实现任务：**

- [ ] 实现 `FrameScheduler`：Active 60、Idle 30、Sleep 15 FPS，静止时无 deadline 则事件驱动。
- [ ] dirty 来源覆盖输入、动画、物理、Brain deadline、resize/DPI、surface 恢复和显式状态变化。
- [ ] 使用 monotonic clock、250 ms accumulator cap 和每轮最多 5 个 fixed step；截断日志节流。
- [ ] 证明 event loop 使用 Wait/WaitUntil，无 busy loop、无无意义 surface present。
- [ ] release 构建分别记录 Walking、Idle、Sleeping/静止的 CPU、GPU、内存、帧率与测量方法。
- [ ] 完成长时间运行测试，记录时长、内存起止/峰值、错误日志和退出结果。
- [ ] 回归 Phase 0-13 自动测试和所有人工场景，修复后重验受影响证据。
- [ ] 核对默认资产许可证、配置错误、资源损坏和 surface 恢复的用户可诊断性。

**自动验证命令：** 执行四条全局门禁命令，并执行：

```bash
cargo build --workspace --release
cargo test --workspace --release
```

运行 frame scheduler 模拟时钟 tests，断言各状态 deadline、静止不 redraw、事件后恢复、累计截断和 FPS 切换不改变行为速度。

**macOS 人工验收：** 用 release 构建完成启动、透明置顶、GLB、Idle、随机 Walk、边缘转身、命中/穿透、点击、拖动、释放落地、Look At Mouse、跨可用 DPI 显示器和退出；记录三档资源数据及长时间运行结果。

**Windows CI 验收：** 候选 commit 的 Windows job 完成 debug/release 构建、全部测试和 Clippy；与 macOS job 同为绿色。

**Windows 实机验收：** 不要求；Windows 门禁仅为 CI 编译、Clippy、测试和构建通过。

**产物与验证证据：** FrameScheduler、性能测试说明、长时间运行日志、最终回归记录，`evidence/phase-14/verification.md`、macOS 完整录屏/截图、性能采样和 CI URL。

**阶段退出条件：** macOS 完整 MVP 场景通过，Windows CI 兼容性门禁通过；正常活动 CPU `< 2%`、Idle 尽可能接近零、内存 `< 150 MB`；无 busy loop 或持续内存增长；所有门禁全绿。任一量化目标未达到时保持 `Verifying`，先更新实现或经评审调整项目预算，不得把 Phase 标记为 `Done`。

**下一阶段解锁条件：** Phase 14 没有下一实现 Phase。标记 `Done` 只解锁最终发布门禁，不自动允许创建 tag。

## 18. 最终发布门禁：v0.1.0-desktop-pet-mvp

Phase 14 为 `Done` 后，使用一个未变更的候选 commit 完成以下清单：

- [ ] Phase 0-14 全部为 `Done`，证据中的 commit 与候选 commit 一致，或有明确的无影响说明。
- [ ] 候选 commit 的 macOS / Windows GitHub Actions 矩阵全绿。
- [ ] 在 macOS 实机从干净配置完成一次完整 MVP 场景。
- [ ] Windows job 对同一默认 GLB 和 manifest 完成编译、Clippy、测试和构建；不据此宣称运行时行为已验证。
- [ ] 资产来源、CC0 许可证、SHA-256 和发布文件清单已复核。
- [ ] 四条全局门禁及 release build/test 在候选 commit 上重新执行。
- [ ] 发布验收记录保存到 `evidence/release-v0.1.0/verification.md`，包含日期、完整 SHA、平台、结果、CI URL、截图/录屏和性能日志路径。
- [ ] 只有以上全部通过后创建 annotated tag `v0.1.0-desktop-pet-mvp`。

在 tag 创建前，AI、网络、商店、多宠物、复杂物理和桌面窗口感知仍然保持禁止开发。
