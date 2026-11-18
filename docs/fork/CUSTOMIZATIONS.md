# ArkeX 个性化改动清单（合并上游时的依据）

ArkeX 是 [Koimsurai/Arke](https://github.com/Koimsurai/Arke) 的改版，发布在
[qqwqqdqqinyt/ArkeX](https://github.com/qqwqqdqqinyt/ArkeX)。本文件列出至今所有经用户确认的
样式优化与功能改动，供每次合并上游（origin/main → atlas-merge）时解决冲突使用。

## 合并规则

1. **与上游冲突时，下列已确认的改动基本按我们的来。** 合并后这些行为、观感和设置项必须仍然存在。
2. **上游实现了相同功能时，谁的实现更好就用谁的**（更稳、更快、更贴近 Windows 原生观感、代码更简洁）。
   采用上游实现时，要把我们这边用户可感知的细节（默认值、数值、文案）补回去。
3. **与我们改动不冲突的上游改动，一般直接采用上游做法**（修复、新功能、重构、依赖升级）。
4. 拿不准的冲突：先按“默认倾向”处理（通常保留我们的行为），并记入
   [`UPSTREAM_UNCERTAIN.md`](UPSTREAM_UNCERTAIN.md)，攒起来一起向用户汇报，由用户定夺。
5. 上游新增的界面文案要补齐 `crates/app/locales/app.yml` 的 en / zh-TW / zh-CN 三语。
6. 发布身份、仓库、产物名等 fork 设置（见最后一节）永远保留我们的。

每条格式：**是什么 / 为什么 / 涉及文件 / 合并原则**。

---

## 一、Atlas 合并（侧栏、预览、图标）

### 1. Atlas 风格侧栏与 Phosphor 侧栏图标
- 是什么：侧栏布局、选中/悬停样式按 Atlas（neondeex/atlas-filemanager）；侧栏条目使用 Phosphor 线性图标（`sb-*.svg`，`Icon::sidebar`）。
- 为什么：用户偏好 Atlas 的简洁侧栏观感。
- 文件：`crates/app/src/workspace_sidebar.rs`、`assets.rs`、`crates/app/assets/icons/sb-*.svg`、`PHOSPHOR-LICENSE`、`THIRD_PARTY_NOTICES.md`。
- 合并：上游改侧栏逻辑（新条目、修复）照收，样式与图标保持我们的。

### 2. 预览窗格（Atlas 式）与文件夹预览
- 是什么：预览窗格按 Atlas 排版；面板内边距 8；文件夹预览为“舞台（封面或选中的文件）+ 详细信息-大 列表”，列表里子文件夹可就地展开（`preview_open`）；单击可预览的图片大图显示，可解码图片单击走 Quick Look（`look_override`）；双击进入文件夹 / 打开文件所在文件夹；列表行与舞台有右键条目菜单（`open_item_menu_at`）。原 `dir_ring.rs` 圆环已于 7d5b5f2 移除。
- 为什么：用户要在预览里直接浏览文件夹内容。
- 文件：`explorer_preview.rs`、`explorer_menu.rs`、`explorer.rs`。
- 合并：上游的预览修复（新格式、Markdown、Office 等）吸收进来，文件夹预览的结构与交互保持我们的。

### 3. 图标：保留 Arke 原分层类型图标
- 是什么：曾试用 Atlas 图标，2026-11-15 决定回到 Arke 原分层 kind 图标（文件夹两层琥珀色、文件纸张 + 彩色字形）。
- 文件：`explorer_view.rs`（`kind_icon`）、`theme.rs`（folder/page 颜色）。
- 合并：上游图标改进可直接用，但不要再引入 Atlas 文件图标。

## 二、缩略图与图标

### 4. Shell 缩略图
- 是什么：Rust 解不了的本地文件（PSD、RAW、HEIC、视频、PDF 等任何有缩略图提供程序的类型）用 Windows Shell `IShellItemImageFactory`（THUMBNAILONLY）取缩略图；`decodes_in_rust` 判断，不用白名单。设置项 `settings.shell_thumbnails`（文件与文件夹页），环境变量 `ARKE_NO_SHELL_THUMBNAILS` 关闭。
- 文件：`media.rs`、`win_icons.rs`/`shell.rs`、`settings.rs`、`workspace_settings.rs`、`app.yml`。
- 合并：上游若做了同样功能，比较稳定性和覆盖面，谁好用谁；设置项保留。

### 5. 图标视图文件夹透出（folder peek）
- 是什么：图标视图里文件夹显示其中第一张图片，夹在文件夹后片与前片之间。**本次（0.5.4）露出比例由约 33% 提高到约 65%**（见第六节）。
- 文件：`explorer_view.rs`（`peek_folder_visual`、`PEEK_FRAME`、`FOLDER_FRONT_TOP`、`peek_reveal`）、`folder_views.rs`/`media.rs`（取第一张图）、`assets/icons/folder-lg-*.svg`。
- 合并：保留我们的。上游若改 `folder-lg-*.svg`，以我们的几何为准（测试 `peek_tests` 会校验 SVG 与常量一致）。

### 6. Windows 系统类型图标
- 是什么：按扩展名/文件夹取 Shell 类型图标（`SHGetFileInfoW` + `USEFILEATTRIBUTES`，不读文件），按显示尺寸请求（`LIST_ICON_PX 48`、`MediaKey::sized_icons`），设置 `settings.system_type_icons` 默认开。
- 文件：`media.rs`（`type_icons`）、`win_icons.rs`、`settings.rs`、`workspace_settings.rs`。
- 合并：保留我们的；上游图标管线改动吸收后确认开关仍生效。

## 三、布局与视图

### 7. 可拖宽侧栏
- 是什么：侧栏宽度可拖动（180–420，默认 248），存 `prefs::SIDEBAR_W` / `settings.sidebar_width`。
- 文件：`workspace_sidebar.rs`、`workspace.rs`、`workspace_rail.rs`、`prefs.rs`、`theme.rs`。
- 合并：保留。上游 0.6 加了可折叠成图标列（Ctrl+B，`theme::sidebar_width()` 返回 `RAIL_W` 或全宽）：两者合并——展开时宽度取 `prefs::sidebar_width()`（用户拖的宽度），折叠动画、悬停展开也用它；图标列用我们的 Phosphor 侧栏图标（`Icon::sidebar()`）。

### 8. 自适应图标网格与 Ctrl+滚轮档位
- 是什么：`explorer_grid.rs` 自适应间距、多行名称、缓存 `GridLayout`；Ctrl+滚轮图标尺寸 48→192 步长 16；继续缩小进入详细信息 S → M → L（见第 12 条）。
- 文件：`explorer_grid.rs`、`explorer_icons.rs`、`explorer_view.rs`、`tests_views.rs`。
- 合并：保留我们的档位；上游的网格修复按功能吸收。

### 9. Tile 宽高比与工具栏缩放
- 是什么：`Tile::for_icon`：图标占满图像区，宽 = 边长 × `settings.tile_width_ratio`%（默认 130）；上下文栏（路径栏下面那一条）按 `settings.toolbar_scale`%（默认 130）缩放，`theme::bar()` / `h_ctx()`。顶栏不随此缩放（见第 17 条）。
- 文件：`theme.rs`、`prefs.rs`、`settings.rs`、`workspace_settings.rs`、`explorer_view.rs`。
- 合并：保留。注意 `workspace_settings.rs` 的 `regroup()` 索引必须覆盖所有卡片（release 下只有 debug_assert）。

### 10. 搜索：默认含子文件夹、搜索独立视图
- 是什么：搜索默认范围“子文件夹”（`Explorer.here_pinned`、`settings.search_subfolders`）；范围条默认收起，开始输入才隐藏列表；搜索结果有自己的视图（`Explorer.search_view`，默认图标），切换视图在搜索中也有效。
- 文件：`explorer_scope.rs`、`explorer_filters.rs`、`explorer_view.rs`（`search_grid_list`/`render_search_tiles`）、`explorer.rs`。
- 合并：保留默认值和独立视图；上游搜索修复吸收。

### 11. 菜单三色调图标（7d5b5f2）
- 是什么：右键菜单图标分三色调：主色（大多数命令）、钢蓝（新建文本、视图、复制路径等少量强调）、琥珀（仅文件夹命令），“新建文件夹”的加号用深琥珀；新增 sort-up/down、folder-plus-body/mark 图标。
- 色值（浅色 / 深色）：`tone_main` #172939 / #b5cfde；`tone_steel` #0f477b / #81cef2；`tone_amber` #c3852a / #f0c864；`tone_amber_deep` #9d6d27 / #e8ca7e。
- 文件：`theme.rs`（Palette）、`explorer_menu.rs`（`Tone`、`menu_glyph`）、`assets/icons/sort-*.svg`、`folder-plus-*.svg`。
- 合并：保留色调与色值；上游新增菜单项给它配上合适的色调。

### 12. 详细信息三级大小（7d5b5f2）
- 是什么：`DetailsSize` Small/Medium/Large（`DetailsMetrics`），Ctrl+滚轮：图标-小 → 详细 S → M → L → 图标-小。
- 文件：`explorer.rs`、`explorer_details.rs`、`explorer_icons.rs`、`settings.rs`、`explorer_viewopts.rs`。
- 合并：保留。

## 四、输入与导航

### 13. 输入框右键菜单与撤销
- 是什么：所有文本框右键菜单（剪切/复制/粘贴/全选等，路径栏多“粘贴并转到”），菜单优先级 50；100 步撤销/重做。
- 文件：`text_input.rs`、`explorer_path.rs`、`app.yml`。
- 合并：保留；上游文本框修复吸收。

### 14. 前进/后退右键历史菜单
- 是什么：右键后退/前进按钮列出历史，点哪跳哪（Atlas）。
- 文件：`explorer_view.rs`、`workspace.rs`（`render_brand_row`）、`explorer_menu.rs`（`open_history_menu`）。
- 合并：保留。

### 15. 幽灵面包屑与路径建议
- 是什么：返回上级后，原来更深的层级以淡色留在当前文件夹后面（`Explorer.crumb_trail`），一点即回；路径建议最多 500 条可滚动 + 访问过的文件夹历史。
- 文件：`explorer_path.rs`、`explorer.rs`。
- 合并：保留。

### 16. 背景右键“排序方式”子菜单
- 是什么：空白处右键用“排序方式”子菜单替换“刷新”。
- 文件：`explorer_menu.rs`。
- 合并：保留。

## 五、本次（0.5.4）四项改动

### 17. 顶栏控件高度统一
- 是什么：主页按钮、后退/前进/向上/刷新、新标签按钮、地址栏、搜索框统一用 `theme::H_TOP`（32 px，= `H_CTL`），在 48 px 顶栏内垂直居中；地址栏与搜索框都带 1 px 内边框，编辑路径时不跳动。顶栏不受 `toolbar_scale` 影响，所以任何缩放下它们都一样高。
- 为什么：用户要求顶部这些控件高度一致。
- 文件：`theme.rs`（`H_TOP`）、`explorer_view.rs`（`ghost_button`、搜索框）、`explorer_path.rs`、`workspace_home.rs`、`workspace.rs`。
- 合并：保留 `H_TOP`；上游新增顶栏控件也用它。

### 18. 文件夹封面露出约 65%
- 是什么：露出比例 =（前片顶边 − 图片框顶）÷ 图片框高。旧：(6.3 − 3.5) ÷ 8.5 ≈ 32.9%；新：前片顶边 8.5、图片框 top 3.4 / 高 7.8 → (8.5 − 3.4) ÷ 7.8 ≈ 65.4%，接近 Windows 资源管理器。纸张层同步下移（top 6.6）。
- 文件：`assets/icons/folder-lg-front.svg`、`folder-lg-lip.svg`、`folder-lg-paper.svg`、`explorer_view.rs`。
- 合并：保留我们的几何。

### 19. 慢速单击重命名
- 是什么：仿 Windows 列表视图：单击已是唯一选中项的条目（名字或整行），之后在系统双击时间（`GetDoubleClickTime`，取不到用 500 ms，限制在 200–5000 ms）内没有别的点击，且鼠标一直停在该项上，就进入重命名。选中那一下不触发；双击、Ctrl/Shift/Alt 点击、多选中的一项、拖拽、右键、按任意键、滚动、鼠标移走、选中变化、窗口/列表先前没有焦点、单击打开模式、搜索结果都不触发或取消。图标视图和详细信息视图共用 `item_shell`。
- 依据：Raymond Chen《How slow do you have to slow-double-click for it to be a rename?》（2008）与《How does Explorer determine the delay…》（2012）：延迟就是双击时间，规则是“选中项上的一次点击，前后双击时间内都没有其他点击”；MS Learn 列表视图 `WM_TIMER` 说明；Finder 的点击改名延迟同样跟随双击速度。
- 文件：`crates/core/src/slow_click.rs`（纯函数状态机 + 单元测试）、`explorer.rs`（`slow_click_*`）、`explorer_view.rs`、`explorer_menu.rs`、`win.rs`（`rename_click_delay`）。
- 合并：上游若实现同样功能，比较后择优；没有则保留。
- **2026-11-18 合并上游 0.6.2：改用上游实现**（上游 #105/#89「再次单击名字重命名」）。理由：同样基于系统双击时间；上游只在点中名字时触发，与资源管理器一致（点行内空白不改名），并处理了首次激活窗口的点击（`first_mouse`）、按键、拖动、搜索结果，且自带完整的 GUI 测试。我们的 `arke_core::slow_click`、`win::rename_click_delay` 已删除；保留的只有右键与滚动时取消（`Explorer::cancel_click_rename`）。

### 20. 地址栏常驻底色
- 是什么：原来悬停时变深的底色（`theme::wash()`）改为常驻底色；编辑路径时（纸色 + 焦点线）不变。与搜索框未聚焦底色一致。
- 文件：`explorer_path.rs`。
- 合并：保留。

## 合并上游 0.6.2（2026-11-18）的其他取舍

- 缩略图：上游新增视频帧缩略图（独立进程 `thumb_host`，更稳）和压缩内存/磁盘缓存。视频改走上游的进程外通道；其他格式仍走我们的 Shell 缩略图（第 4 条），并接入上游的磁盘缓存。上游“没有主题图标的类型用注册程序图标”在关闭 Windows 类型图标时生效；开启时仍用我们的系统类型图标（第 6 条）。内存解码缓存采用上游更小的预算（16 MB + 48 MB 压缩）。
- 预览窗格：采用上游的“短窗口时画面让高度、详情可滚动”、多页 PDF 与幻灯片滚动、详情行图标；保留 Atlas 顺序（标题在上）和我们的标题排版、悬停才显示的操作按钮。
- 路径建议：保留 500 条 + 历史；采用上游“当前文件夹名过长时截断”。
- 图标视图名称：采用上游的 `tile_label`（点名字重命名需要），保留我们的三行与字号。
- 上游新增 workflow、README 截图与多语言 README、arm64 构建、发布证明等不引入（第 22 条）。

## 六、更新器与发布 / fork 设置

### 21. 更新器修复
- 是什么：下载时边下边算哈希、重命名失败重试（杀毒软件短暂锁文件）、清理残留的部分下载。
- 文件：`crates/app/src/update.rs`。
- 合并：上游若修了同一问题，择优；否则保留。

### 22. 发布与 fork 设置
- 是什么：`release.toml` / `build_info.rs` 指向 qqwqqdqqinyt/ArkeX（CI 中 xtask 设置 `ARKE_RELEASES_REPO`）；产物 `arkex-<版本>-setup.exe`（安装包）和 `arkex-<版本>.exe`（便携版），仅 x64；安装包 AppName ArkeX、独立 AppId；README 一行；`.github` 只保留 `release.yml`（测试不阻塞）；提交身份 qqwqqdqqinyt <322607286+qqwqqdqqinyt@users.noreply.github.com>。发布：本地 atlas-merge 上 squash（`git commit-tree HEAD^{tree} -p arkex-main`）推到 ArkeX main，再推 `vX.Y.Z` tag 触发 `release.yml --arch x64`。小修不升版本，覆盖当前 release。
- 文件：`release.toml`、`crates/app/src/build_info.rs`、`installer/`、`.github/workflows/release.yml`、`tools/xtask`、`README.md`。
- 合并：永远保留我们的；上游新增的 workflow 默认不引入（除非用户要）。

### 23. 其他
- 构建：`third_party/gpui-pre-windows/prebuilt_shaders_bytes.rs` 只在本地构建时用，绝不提交；build.rs 的临时改动构建后还原。
- 文案：新界面文字一律进 `crates/app/locales/app.yml`，en / zh-TW / zh-CN 三语。

### 24. 详细信息尺寸统一、系统文件夹缩略图、小版本不弹“新功能”（0.5.4 覆盖版）
- 是什么：详细信息三档每档文件/文件夹/缩略图同尺寸（16/24/32，额外行高 0/4/12，大档有缩略图），仿 Finder 列表视图；图标视图含图片的文件夹在开启 Windows 类型图标 + 系统缩略图时用 Shell 文件夹缩略图（`MediaKey::shell_folder`、`explorer_view.rs::Peek`），取不到退回自绘透图；修订号更新（major.minor 相同）不自动打开“新功能”页（`whats_new.rs::on_startup`）。
- 文件：`crates/app/src/explorer.rs`（DetailsMetrics）、`explorer_view.rs`、`media.rs`、`whats_new.rs`。
- 合并：保留我们的；上游若改“新功能”弹出规则，保留“修订号不弹”。
