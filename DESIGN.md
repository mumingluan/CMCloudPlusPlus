---
"name": "CMCloud++"
"description": "紧凑、蓝色重点操作，支持浅色、深色与系统主题的文件工作台"
"colors":
  "accent": "#2861dc"
  "accent-hover": "#184fc3"
  "text": "#263246"
  "muted": "#596a82"
  "border": "#e6eaf1"
  "surface": "#fff"
  "canvas": "#f3f5f9"
  "surface-soft": "#f5f7fb"
  "nav-hover": "#e9edf5"
  "nav-active": "#e6edfc"
  "nav-text": "#58677f"
  "nav-active-text": "#245acb"
  "secondary-text": "#4f6079"
  "secondary-border": "#dce2eb"
  "quiet-text": "#576a86"
  "quiet-hover": "#eff3fb"
  "field-border": "#dce3ee"
  "file-text": "#354761"
  "row-hover": "#f8faff"
  "row-selected": "#eff5ff"
  "row-focused": "#f3f7ff"
  "chip-surface": "#edf3fd"
  "chip-text": "#4b6a9b"
  "red": "#bc3544"
  "red-hover": "#a62938"
  "error-surface": "#fdf0f2"
  "error-text": "#9d3041"
  "progress-track": "#edf1f7"
  "progress-value": "#477de5"
  "repository-shadow": "rgb(0 0 0 / 0.18)"
  "repository-backdrop": "rgb(8 18 37 / 0.48)"
"typography":
  "headline":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "27px"
    "fontWeight": 650
    "letterSpacing": "-0.025em"
  "headline-mobile":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "23px"
    "fontWeight": 650
    "letterSpacing": "-0.025em"
  "title":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "19px"
    "fontWeight": 600
    "letterSpacing": "-0.01em"
  "body":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "14px"
    "fontWeight": 400
    "lineHeight": 1.8
  "label":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "14px"
    "fontWeight": 500
  "metadata":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "12px"
    "fontWeight": 400
  "repository-title":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "23px"
    "fontWeight": 650
    "letterSpacing": "-0.01em"
  "repository-heading-mobile":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "20px"
    "fontWeight": 650
    "letterSpacing": "-0.025em"
  "repository-dialog-title":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "21px"
    "fontWeight": 600
    "letterSpacing": "-0.01em"
  "offline-heading":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "24px"
    "fontWeight": 650
    "letterSpacing": "-0.025em"
  "offline-heading-mobile":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "22px"
    "fontWeight": 650
    "letterSpacing": "-0.025em"
  "section-title":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "20px"
    "fontWeight": 600
    "letterSpacing": "-0.01em"
  "record-title":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "16px"
    "fontWeight": 600
    "letterSpacing": "-0.01em"
  "summary-value":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "18px"
    "fontWeight": 400
  "supporting":
    "fontFamily": "'Segoe UI', 'Noto Sans SC', 'Microsoft YaHei', sans-serif"
    "fontSize": "13px"
    "fontWeight": 400
"rounded":
  "tag": "4px"
  "field": "6px"
  "control": "7px"
  "table": "8px"
  "panel": "10px"
  "dialog": "11px"
  "pill": "12px"
"spacing":
  "4": "4px"
  "6": "6px"
  "8": "8px"
  "10": "10px"
  "12": "12px"
  "16": "16px"
  "20": "20px"
  "24": "24px"
  "32": "32px"
"components":
  "button-primary":
    "backgroundColor": "{colors.accent}"
    "textColor": "{colors.surface}"
    "typography": "{typography.label}"
    "rounded": "{rounded.control}"
    "padding": "0 15px"
    "height": "37px"
  "button-primary-hover":
    "backgroundColor": "{colors.accent-hover}"
  "button-secondary":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.secondary-text}"
    "typography": "{typography.label}"
    "rounded": "{rounded.control}"
    "padding": "0 15px"
    "height": "37px"
  "button-secondary-hover":
    "backgroundColor": "{colors.surface-soft}"
  "button-quiet":
    "backgroundColor": "transparent"
    "textColor": "{colors.quiet-text}"
    "typography": "{typography.label}"
    "rounded": "{rounded.control}"
    "padding": "0 10px"
    "height": "34px"
  "button-quiet-hover":
    "backgroundColor": "{colors.quiet-hover}"
  "button-destructive":
    "backgroundColor": "{colors.red}"
    "textColor": "{colors.surface}"
    "typography": "{typography.label}"
    "rounded": "{rounded.control}"
    "padding": "0 15px"
    "height": "37px"
  "button-destructive-hover":
    "backgroundColor": "{colors.red-hover}"
  "field-input":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.text}"
    "rounded": "{rounded.field}"
    "padding": "10px 12px"
  "navigation-item":
    "backgroundColor": "transparent"
    "textColor": "{colors.nav-text}"
    "rounded": "{rounded.table}"
    "padding": "0 13px"
    "height": "46px"
  "navigation-item-active":
    "backgroundColor": "{colors.nav-active}"
    "textColor": "{colors.nav-active-text}"
  "space-tag":
    "backgroundColor": "{colors.chip-surface}"
    "textColor": "{colors.chip-text}"
    "rounded": "{rounded.tag}"
    "padding": "3px 7px"
  "panel":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.text}"
    "rounded": "{rounded.panel}"
    "padding": "25px"
  "file-row":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.file-text}"
    "height": "48px"
  "file-row-selected":
    "backgroundColor": "{colors.row-selected}"
  "transfer-details":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.text}"
    "rounded": "{rounded.dialog}"
    "padding": "23px"
    "width": "460px"
  "repository-mark":
    "textColor": "{colors.accent}"
    "width": "18px"
    "height": "18px"
  "repository-dialog":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.text}"
    "rounded": "{rounded.dialog}"
    "padding": "24px"
    "width": "100%"
  "repository-credential-dialog":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.text}"
    "rounded": "{rounded.dialog}"
    "padding": "24px"
    "width": "440px"
  "repository-plan":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.text}"
    "rounded": "{rounded.dialog}"
    "padding": "24px"
    "width": "640px"
  "offline-task-summary":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.text}"
    "padding": "17px 4px 17px 24px"
---

# Design System: CMCloud++

## Overview

**Creative North Star: "井然有序的文件桌"**

「井然有序的文件桌」把视觉重心交给文件名称、选择状态与操作结果。界面以明亮的白色工作面和灰蓝色导航构成稳定背景，用清晰的蓝色标识主要操作与当前位置；密度紧凑，留白服务于扫描和触达。

组件使用精确边界、轻度圆角和系统无衬线字体。桌面保留高效的行列关系，手机把相同信息收束为单列，并扩大交互区域。深度主要来自色面和分隔线，浮层才使用阴影；运动只解释状态变化，尊重减少动态效果的设备偏好。

**Key Characteristics:**

- 白色工作面、灰蓝色导航、清晰的蓝色重点操作。
- 紧凑文字层级与规整行列，数字使用等宽数字特性。
- 轻度圆角、细分隔线，浮层具有明确层级。
- 手机控件保持可触达，文件操作通过统一菜单展开。

当前规范从 `src/styles.css` 与 `src/App.tsx` 提取；现有桌面、手机、选择、登录和传输详情截图位于 `.impeccable/review/`。

AxuanStor、对象库设置与离线下载同时依据 `src/RepositoryWorkspace.tsx`、`src/RepositorySettings.tsx`、`src/RepositoryDialog.tsx`、`src/repositoryFiles.ts`、`src/OfflineDownloads.tsx` 及对应样式记录，延续同一文件桌、浅色 / 深色 / OLED / 跟随系统外观。对象库与普通文件共用 `App.tsx` 的浏览工作面、操作对话框和传输队列。当前前端证据位于 `R:/Temp/CMCloud-Unified-20261111/ui-review/`；本轮[最终评审](R:/Temp/CMCloud-Unified-20261111/ui-review/reviewer-verdict.md)将破坏性操作对比度、共享移动操作目标、演示标识与文档一致性四项修正判为 **ship**，范围为 React / headless Chromium fixture 中的这四项修正及其相关回归。Windows 原生 GUI 验收仍按用户要求推迟；实体 Android、原生安全存储、密码学强度与实际提供商操作须分别验收。

## Colors

冷白与灰蓝构成安静背景，操作蓝提供明确的行动信号。前面的 token 为规范值，sidecar 中的色阶仅用于面板预览。

### Primary

- **操作蓝**（`accent`）：上传、确认、保存和连接等主要操作，以及选择数量和焦点。
- **深操作蓝**（`accent-hover`）：可用主要按钮的悬停反馈。
- **浅导航蓝 / 选中蓝**（`nav-active`、`row-selected`、`row-focused`）：当前导航、文件选择与键盘焦点。
- **标签蓝**（`chip-surface`、`chip-text`）：云端、只读等短状态标签。
- **进度蓝**（`progress-value`）：细进度条中的完成部分。

### Neutral

- **工作白**（`surface`）：主要工作面、表单和面板。
- **导航灰白**（`canvas`）：桌面侧栏与外层背景。
- **柔灰白**（`surface-soft`）：搜索、辅助容器及次要按钮悬停。
- **深石板文字**（`text`、`file-text`）：正文、标题和文件名。
- **辅助灰蓝**（`muted`、`nav-text`、`secondary-text`、`quiet-text`）：说明、导航和次要操作。
- **细边界灰**（`border`、`secondary-border`、`field-border`）：面板、按钮与字段边界。
- **行悬停浅白**（`row-hover`）：文件行的轻度反馈。
- **进度底灰**（`progress-track`）：完成部分之外的任务进度。
- **对象库浮层阴影 / 遮罩**（`repository-shadow`、`repository-backdrop`）：对象库新建、管理、缓存设置、凭据确认和上传比对浮层的深度与背景隔离，保留源码的透明 RGB 表达。

删除确认使用 `red` 与 `red-hover`；可读失败详情使用 `error-surface` 与 `error-text`。这些是操作语义色，保持文字说明。

**The Action Blue Rule.** 蓝色用于主要操作、当前导航、选择数量和键盘焦点；状态文字同时写明含义。

## Typography

**Body Font:** Segoe UI；中文依次回退到 Noto Sans SC、Microsoft YaHei，再回退到 sans-serif。标题、正文、标签使用同一字体族。

**Character:** 系统字体让中文文件名与本机界面保持熟悉的阅读节奏。层级由字号、字重和留白构成，辅助信息继续保持可读。

### Hierarchy

- **Headline:** 工作区标题使用 `headline`；手机切换为 `headline-mobile`。
- **Title:** 面板标题使用 `title`；对话框标题实际为 18px，手机为 17px。
- **Body:** 正文以 `body` 为基础；段落行高较宽，适合说明和错误阅读。
- **Label:** 操作按钮和文件名以中等字重为主；文件名在桌面为 14px，手机为 13px。
- **Metadata:** 文件类型、路径、时间和传输数字使用 `metadata`；手机次级信息为 11px。文件大小、修改时间和传输数字使用 `tabular-nums`。

对象库打开与解锁状态保留任务标题：打开前的对象库名称在桌面使用 23px，手机使用 `repository-heading-mobile`；进入后由共享面包屑呈现当前位置。新建、解锁及解锁方式修改标题使用 `repository-title`，管理和上传比对标题使用 `repository-dialog-title`。离线页标题使用 `offline-heading` / `offline-heading-mobile`，连接分区使用 `section-title`，节点与记录标题使用 `record-title`。比对和状态汇总数字使用 `summary-value`，表单标签与说明使用 `supporting`，常规名称与辅助数据继续使用 `body` / `metadata`。这些是现有增量界面的有意层级。

**The Readable Metadata Rule.** 辅助文字、占位文字和状态文字保持清晰；普通正文与其背景的对比度至少为 4.5:1。

## Layout

工作区采用可伸缩的主区域，文件与传输列表在各自内容区滚动。共享间距集中在 4、6、8、10、12、16、20、24、32px；具体页面使用实际布局尺寸。

- **桌面：** 侧栏宽 228px，顶部条高 68px，工作区内边距为 32px 32px 22px。文件表以名称列吸收剩余空间，文件大小和修改时间按列对齐。
- **宽屏（至少 1600px）：** 工作区内边距为 38px 44px 28px。
- **紧凑桌面（至多 1150px）：** 侧栏收为 205px，工作区内边距为 25px 24px 18px。
- **中间宽度（至多 900px）：** 账号页保持单列任务布局，文件行的更多操作持续可见。
- **手机（至多 700px）：** 侧栏隐藏，顶部条为 58px 加顶部安全区，主区内边距为 18px 17px 15px。底部导航为 65px 加底部安全区，内容为它预留空间。
- **手机文件表：** 名称与大小保留，修改时间列隐藏；表格横向贴近屏幕边缘。工具栏可换行，搜索占独立整行。
- **列表密度：** 普通目录与对象库共用文件行：桌面紧凑模式为 48px，宽松模式和手机为 72px；图标视图每行 152px。传输列表按 120px 行高组织。选择状态在原工具栏位置显露批量操作。
- **触达：** 手机图标按钮、选择区域、主要表单字段、列表导航与操作按钮至少提供 48px 的交互尺寸；传输控制间距为 8px。
- **对象库：** 复用普通文件的工具栏、面包屑、虚拟列表 / 图标视图、选择、搜索、筛选、排序和项目计数；进入后显示「管理对象库」「退出对象库」，隐藏「新建对象库」。手机沿用相同的单列文件行和底部导航。
- **离线下载：** 连接表单与已存连接在桌面可并排，派发区最大宽 960px；手机收为单列。状态导航可横向滚动，任务展开内容在手机使用 96px 标签列与可折行的值列。
- **新增移动控件：** 至多 700px 时，对象库与离线页按钮至少为 48 × 48px；输入、选择器、文本域、记住信息标签、任务与节点展开入口至少为 48px 高。桌面保留原有紧凑尺寸。

## Elevation & Depth

深度采用色面分层与细边界，常规文件行和面板保持平稳。对话框与提示浮在内容之上，背景遮罩使任务焦点清楚。

### Shadow Vocabulary

- **Dialog:** `0 18px 70px #14294435`，用于操作、传输详情和预览对话框。
- **Toast:** `0 8px 28px #18274024`，用于临时结果提示。
- **Backdrop:** `#263c5d55`，用于原生对话框后方。
- **Repository dialog:** `0 16px 48px`，颜色使用 `repository-shadow`；对象库原生 modal 承担阴影，其内部表单清除重复阴影。
- **Repository backdrop:** 使用 `repository-backdrop`，用于对象库新建、管理、缓存设置、凭据确认与上传比对 modal 后方。

**The Quiet Surface Rule.** 文件行、登录面板和设置面板以色面和边界分层；阴影集中于对话框与提示。

状态色变化通常持续 0.16s，文件行反馈持续 0.12s，开关持续 0.18s。读取旋转与骨架脉冲表达进行状态；减少动态效果时关闭动画和过渡。

## Shapes

形状使用轻度圆角与细边界。短标签采用 `tag`，表单采用 `field`，常规按钮采用 `control`，文件表采用 `table`，登录和设置容器采用 `panel`，对话框采用 `dialog`。头像和连接提示保留圆形形态。

面板与字段使用单像素边界；手机文件表减去左右外框，并维持内部行分隔。文件类型使用小型轮廓图标，文件夹带有暖黄色填充。图标与文字承担工作界面的层级。

## Components

### Buttons

明确、紧凑的操作控件。主要、次要、安静和删除确认是四个实际变体。

- **Primary:** 操作蓝底与白字，可用状态悬停时切换深操作蓝。
- **Secondary:** 白底、灰蓝文字和细边界；悬停时轻度改变底色与边界。
- **Quiet:** 透明底、灰蓝文字，适合批量工具栏与辅助操作；悬停使用淡色面。
- **Destructive:** 删除确认使用独立的 destructive 变体及其主题映射，浅色为红底白字；删除入口使用语义色文字。清除全部凭据的两次确认沿用此变体。
- **Focus / Disabled:** 键盘焦点使用蓝色 2px 轮廓与 3px 偏移；按钮禁用不响应操作，主按钮和图标按钮最终透明度为 0.55。手机常用操作扩大为 48px 高。

### Inputs / Fields

字段使用白底、细边界、轻度圆角和 10px 12px 内边距。标签与字段之间留 9px，说明文字放在字段下方。键盘焦点使用共享轮廓；搜索在外容器变更边界颜色，保持内部输入无多余轮廓。手机输入与选择器至少为 48px 高，文本域至少为 96px 高。

操作表单在提交时显示「处理中…」，禁用确认、取消和字段。名称输入的 Enter 与点击确认共享提交锁，并跳过输入法组合过程。

### Navigation

导航保持相同文字与轮廓图标。桌面导航使用横向图标、文字和可选任务计数，当前位置带浅蓝色面与更深蓝字。手机导航固定在底部，图标在文字上方；当前图标线条略加强，按钮至少为 48px 高。

### Chips / Tags

云端与只读等状态采用紧凑浅蓝标签，短文字配小圆角。传输计数用操作蓝底和白字。标签解释状态，交互继续由明确按钮承担。

### Cards / Containers

登录与设置面板以白色内容、细边界和 `panel` 圆角组织，桌面内部留 25px。手机登录面板为 19px 15px，设置面板为 19px 16px。面板层级来自边界和内部文字层级。

### File Rows & Batch Actions

文件名、类型、大小与时间形成可扫描的行。工具栏按钮切换图标或列表视图并保存偏好；图标视图按容器宽度分列，每行高 152px，两种视图都使用虚拟滚动。选择行使用浅蓝背景，并在工具栏显示批量操作。

桌面支持鼠标拖框；触屏从文件区空白处拖动也可框选，抬手后保留已选文件与批量工具栏。文件行上的触摸用于原生滚动。手机长按文件 450 ms 进入选择模式，移动超过 10px 会取消尚未触发的长按；选择模式中可从复选框滑动连续选择。选择手势触及列表边缘时自动滚动。

右键、键盘菜单键和三点按钮共用文件操作菜单；菜单保留已选择的多个文件，分享空间仅显示只读操作。菜单避开视口边界，支持方向键、Escape 和点击外部关闭。界面禁用默认右键菜单与普通文字选择，输入字段继续支持文字编辑。

### Transfer Ledger & Details

任务行保留名称、文字状态、细进度条、数量信息和恢复操作。点击任务名打开详情，将完整名称、状态、进度、本地路径和失败原因放在可滚动对话框内。失败区采用浅红面与深红文字；路径和失败原因保留换行、任意长字符串折行与选择复制。

### Operation Dialogs

普通目录与对象库共用 OperationDialog 完成新建文件夹、重命名、删除、复制和移动；对象库操作通过适配器提交至对象库 API。对话框默认宽 460px，最大宽为视口减 28px，最大高为动态视口减 40px，并允许滚动。目标目录显示面包屑、文件夹列表与读取状态；目录切换隔离迟到响应。确认按钮在读取目录或操作进行中保持禁用，连按 Enter 仅启动一次操作。手机布局中共享对话框的页脚按钮至少为 48 × 48px。

### AxuanStor Repositories

已确认的个人空间对象库在文件夹图标右下角显示蓝底白色 A 标记；标记为内联 SVG，使用 `repository-mark` 尺寸，外沿以工作面颜色隔开。默认打开后由 `App.tsx` 的同一文件浏览器展示逻辑目录；文件夹三点菜单提供「打开 AxuanStor 对象库」「管理对象库」和「以普通文件夹打开」，并保留原有文件夹操作。`RepositoryWorkspace.tsx` 负责会话、打开 / 解锁、管理与上传比对；文件行、图标视图、选择、菜单和常规操作由共享浏览器承担。进入对象库后，共享工具栏提供「管理对象库」「退出对象库」，并隐藏「新建对象库」；离线下载继续作为独立导航目的地。

对象库外的可写个人空间在桌面工具栏和手机「添加」菜单提供「新建对象库」。单列表单先填写名称，再选择普通或加密类型，默认普通。加密创建与解锁方式修改复用密码 / 密钥切换：密码需要再次确认，不一致时在表单内报错；密钥可输入十六进制或 Base64 文字、导入文件，或「生成并保存新密钥」。导入和生成后的选择以文件名呈现，界面接收不透明选择标识。

加密入口提供密码、密钥文字和导入密钥文件三条路径。「记住密码 / 在本机保存密钥」默认未选中，旁边说明系统安全存储；已保存信息有单独的使用和移除操作。对象库读取、校验或传输、错误、空目录、历史快照与冲突均使用明确文字反馈。普通对象库首次读取时提供「取消并返回」，失败后提供「重新读取」。

「管理对象库」先打开对象库，加密库先解锁，再显示名称、类型、对象库 ID 和已设置的解锁方式；共享文件工具栏也有带标签的管理入口。ID 使用辅助字号和等宽数字，长值允许折行。支持修改凭据的加密库提供「修改密码」和「更换密钥」，保存后返回管理摘要；已保存的信息可单独清除。早期固定解锁格式显示迁移要求；修改说明保留已解锁设备、旧密钥及历史副本的访问边界。

新建、管理及解锁方式修改使用 `repository-credential-dialog` 的紧凑宽度，且不超过视口减 32px。上传确认针对全部选择项显示与已有内容、本批内容的比对结果：相同路径跳过、复用已有内容、复用本批文件、上传新内容。汇总区分别呈现逻辑文件大小、新增内容份数和实际需上传大小；每页 50 项仅限制结果浏览。替换已有路径需要另行勾选确认。上传比对 modal 最大宽 640px，且不超过视口减 32px；内容填满外层宽度，最大高为动态视口减 48px。手机管理面板内边距为 20px，表单保持单列，控件沿用 48px 触达尺寸。

对象库上传与下载进入现有传输队列，沿用文字状态、进度、暂停 / 继续、取消和任务详情。加密 / 解密作为队列中的可读阶段呈现；这些界面状态本身不构成原生字节处理或云端写入验收。

### Repository Settings & Local Preview

设置沿用平面偏好分区，增加「AxuanStor 对象库」。缓存管理 modal 按对象库显示名称、可折行 ID、占用大小与「删除缓存」；使用中的缓存说明须先退出对象库，并禁用删除。桌面记录横向排列，手机将身份、大小与动作收为单列；保留读取、空结果、错误和刷新状态。

「删除全部凭据」先说明会移除本机保存的对象库密码、密钥与协调器令牌并退出已打开对象库，再要求第二次明确确认。最终确认使用共享 destructive 变体；提交中保持取消与确认锁定，结果显示在所属设置分区。

对象库预览复用现有预览浮层。需要解密的文件先通过传输队列准备本机预览，准备完成后再显示内容；暂停状态说明从传输列表继续。临时解密源禁用链接复制并解释仅供本机使用、关闭后回收，关闭浮层时发出释放请求。临时明文的原生清理、崩溃恢复和安全存储仍需各自验收。

浏览器 `preview=scale` fixture 在主工作面持续显示「示例数据 · 界面预览」，管理与上传比对等 modal 内另有示例标识；真实 Tauri 会话不显示 fixture 提示。界面预览与原生 / 云端验收边界保持分明。

### Offline Downloads

离线下载是独立导航目的地。连接表单使用协调器 URL、密码式管理员令牌字段和默认未选中的保存选项；保存说明、已存连接的使用、断开与移除操作均可见。派发区选择对象库与逻辑目录，每行一个 HTTP / HTTPS 地址，显示 100 个地址的数量上限。

状态区提供任务、节点、对象库、存储后端、批次与事件六个视图，保留刷新时间、读取反馈、失败后的上次成功状态及空结果。任务摘要使用文字状态、进度和分配节点；展开后可读任务标识、对象库 / 批次、完整校验和、原上传会话是否保留、确认分片、尝试 / 历史、更新时间与失败代码。节点预算使用 B、KiB、MiB、GiB，速度带每秒单位。任务筛选与分页保持明确。演示数据始终有文字标识；状态字段的前端呈现不代表原生脱敏或服务端行为已验收。

### Repository Dialog Focus

对象库新建、管理、解锁方式修改、缓存管理、凭据确认与上传比对使用原生 modal。首次具备可用控件时设置初始焦点：新建的名称输入，管理及解锁方式修改的首个可用控件，上传比对和缓存管理的关闭动作，凭据确认的取消动作。此后忙碌状态切换不重新夺取焦点。打开前保留触发控件；管理与上传比对关闭后等异步刷新及共享工具栏解除忙碌再恢复焦点，触发项已消失时落到可用且有标签的「管理对象库」控件。常规文件操作继续使用共享 OperationDialog。

创建、保存解锁方式、清除已存信息或等待密钥导入 / 生成选择器时，相关字段、类型与解锁方式切换、记住选项、提交和取消动作保持禁用，已提交的解锁方式保持可见。创建与管理 modal 在提交或选择器未结束时阻止 Escape 和返回关闭；应用返回事件先交由当前 modal 的取消处理，工作区返回同时遵守选择器锁定。请求结束后恢复可用操作。键盘轮廓继续使用 2px 操作蓝与 3px 偏移。

## Do's and Don'ts

### Do:

- **Do** 用蓝色明确主要操作、当前导航和键盘焦点，并保留文字状态。
- **Do** 在手机保持至少 48 × 48px 的交互区域；传输控制按钮之间保留 8px 间距。
- **Do** 保持普通文字至少 4.5:1 的前景与背景对比度，辅助文字使用清晰的灰蓝色。
- **Do** 让完整文件名、路径和失败原因在详情中换行、滚动和选择复制。
- **Do** 在读取目录和提交操作时显示进行状态；当前目录仅接受对应请求结果，提交中的确认操作保持锁定。

### Don't:

- **Don't** 让截断的传输摘要成为失败原因的唯一入口。
- **Don't** 用过浅的文字表达辅助信息或依赖颜色单独表达任务状态。
- **Don't** 缩小手机交互区域，或让相邻传输控制按钮贴合。
- **Don't** 让迟到目录响应覆盖当前位置，或让连续 Enter 在进行中的操作上重复提交。

## Account workspace

会话管理合并至设置，账号主文字直接展示手机号。桌面侧栏提供文件、传输、分享三项工作导航并可收缩；手机使用文件、传输、分享、设置四项底部导航。

账号内容采用最大 960px 的单列工作面。当前连接首先展示账号摘要与空间状态，容量使用情况紧随其后；空间切换在独立表单中选择目标、填写群组 ID 或分享链接并确认连接。更换账号表单按需展开，未登录时直接显示。登录方式使用带标签的选择器，短信验证与高级字段逐步展开。

账号名称为 18px，任务标题为 17px；手机账号名称为 16px，工作区移除页面顶部标题。分区用留白与细横线组织。手机控件保持至少 48px 高，表单单列排列；错误紧邻所属表单，成功连接后进入文件列表。


## Navigation and appearance (0.1.3)

The desktop sidebar is 218 px wide, collapses to a 68 px rail, and keeps settings at the bottom. Session management is a section within settings. Phones use four bottom destinations with 48 px controls. File content starts directly in the working area. Account identities appear once in the saved session list, with explicit connect, disconnect, and credential removal actions.

Light mode preserves the existing white workspace and slate navigation. Dark mode uses #141e2e working surfaces, #e3eaf5 primary text, #b4c1d5 secondary text, #33415a seams, #83adff links, and #306adc primary buttons with white labels. System mode responds to OS changes; desktop window chrome follows the chosen theme. Preferences persist locally.

Uploads always compute the full SHA256 before starting or resuming an upload. Resumed uploads validate both their stored checksum and source metadata.

Directory updates preserve the loaded working set and pagination during automatic synchronization. Completed uploads are published together in 150 ms batches, then reconciled within a fixed 1.5 s window; an in-flight listing finishes before another automatic request starts. Successful deletion results update the rows immediately, while errors remain actionable on the failed items.

## 2026-11-06 传输与导航更新

传输页按上传中/已上传、下载中/已下载、全部进行中/全部已完成分成三组。仅进行中显示右侧错误筛选；总进度位于分类下方，深紫/浅紫表示上传完成/剩余，深绿/浅绿表示下载完成/剩余，混合视图按字节占比排列四段，下方显示双向总速度。任务数量和分页由方向、状态及错误条件共同确定。

品牌区为静态标识；侧栏按钮显示“收起”。标题显示当前账号手机号。文件设置提供本地持久化的厂商目录开关，只影响根目录的四个指定文件夹。Windows 托盘使用应用图标，关闭窗口隐藏至托盘；状态与批量传输操作在托盘菜单可达。

传输分类按容器实际可用宽度自动适配：能容纳时六项分组横排，容纳不足时使用分组下拉菜单；宽度变化保持当前分类与错误筛选状态。


## Compact workspaces and touch browsing (2026-11-06)

Page titles are removed; desktop workspaces start with 12 px top padding and functional toolbars. Upload file, create folder and upload folder share the quiet button treatment. Blank directory space offers refresh, folder upload, file upload and create folder through the shared menu; read-only shares offer refresh.

Phones use a search-first toolbar with Add and Refresh. Holding a file for 450 ms enters selection mode and selects that file; scrolling cancels a pending hold. Add and file menus open above the bottom navigation with a dimmed backdrop. Normal taps open files and folders; selection mode toggles items and enables checkbox swipe selection. File actions remain available from the three-dot menu. Bottom navigation marks the current destination with icon and text weight/color. The mobile loaded-item count is centered in its footer. The 72 px list row gives names and metadata separate lines; mobile file names use 14 px text. Virtual row transforms remain dedicated to positioning.

Navigation transitions are directional and last 180 ms, bottom navigation uses an active label and a small top indicator, menus open in 130 ms, and dialogs in 160 ms. The first render has no navigation animation. System reduced motion disables these transitions.


## Settings and OLED palette (2026-11-06)

The settings overview has one account entry, followed by Appearance, Transfers and File Browsing groups. Saved sessions, cloud-space selection and login are within the account entry, with an explicit return action. Groups use flat preference rows; the save action stays at the bottom of the scroll container. Saving options updates account metadata in place and retains the current settings route and file location. Failed preference reads expose retry.

Mobile file names, settings row labels and transfer names use 14 px platform system text; auxiliary metadata uses 12 px. List rows remain 72 px and action targets remain at least 48 px. Automatic browser text inflation is disabled while the operating-system font family is retained.

Appearance offers Follow System, Light Mode and Dark Mode. The effective dark appearance has a separate OLED Pure Black switch. Follow System reacts to OS appearance changes without overwriting the selected preference. OLED uses #000000 workspace and navigation while retaining the dark theme’s colored controls, file icons, upload/download progress segments and status colors. Images and videos retain their original colors. Preferences persist locally and apply immediately. Light Mode ignores the OLED flag without clearing it; previous appearance preferences migrate on first use.


## Pending preference feedback

The Save Settings button is neutral gray and disabled until a preference differs from its last confirmed saved value. Unsaved rows receive a subtle full-row tint without moving their content; OLED retains the dark theme’s pending-change tint. Reverting a value clears its marker immediately. Successful persistence advances only the confirmed values, so failed saves and edits made during a request remain pending. Appearance mode, the OLED switch and vendor-folder visibility retain their immediate local persistence and do not create unsaved server preferences.


## Startup presentation

The desktop renderer paints before initializing the tray. Initial transfer summary and pagination reads run outside the native UI thread. The file workspace does not poll transfer overview; the transfer workspace and tray retain their live progress updates. Native tray labels and enabled states update only when their displayed values change.


## Mobile toast placement

Mobile toasts use safe-area-aware left/right insets and no horizontal transform. Their content can wrap and scroll within the viewport; the close action and status icon retain their width. The bottom inset clears the mobile navigation and gesture area.


Uploads always report their actual source-file size. There is no size-reporting preference; legacy saved fields are ignored on loading, and upload creation uses the size obtained from file metadata.


## File search and navigation

Search filters the loaded working set until the user explicitly requests recursive search. When a query has no matches or the current folder is exhausted, the list ends with a recursive-search action; that action traverses paginated subfolders, reports progress and can be stopped. Clearing the query or changing directory cancels the recursive view. Results preserve their ancestor folders for navigation. The loaded-item count belongs at the end of the scrolling list.

Name, modification time and size sorting share the same ascending/descending controls in list and icon views, with folders grouped first. Icon names may wrap to two lines; folder cards omit size. Android Back dismisses overlays or selection first, then returns to the parent directory; at the root it backgrounds the task to keep transfers alive.


## Mobile file controls

The file toolbar has icon-only Add and Display Options actions. Focusing search replaces Add with Close Search; clearing text inside the field keeps search active. Display Options groups list/icon mode, sort key/direction and file type into one bottom sheet. Pulling down from the top of a file list refreshes the current directory. Empty search results fit the visible content area and show the recursive-search action directly.


## Desktop upload drop target

Native desktop file drags highlight the current file workspace with an inset dashed accent boundary, target folder name and source count. The target is active only for writable file browsing, with physical drag coordinates mapped to CSS pixels for high-DPI displays. Dropping enqueues uploads to the current directory, preserves folder hierarchy and uses the existing cancellable scan/transfer workflow. Leaving, switching directories or opening a modal clears the drop target.


## Transfer batch actions

Ongoing transfer views show Pause, Start and Cancel as one compact action row. Labels and scope follow All, Upload or Download; availability uses full queue summary counts rather than the visible page. Cancel opens a focused confirmation dialog with task count, pagination scope and download-checkpoint cleanup behavior. Keeping tasks receives initial focus. Mobile actions share one row and maintain at least 44px touch height; confirmation actions use 48px targets.
