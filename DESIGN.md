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
    "height": "64px"
  "file-row-selected":
    "backgroundColor": "{colors.row-selected}"
  "transfer-details":
    "backgroundColor": "{colors.surface}"
    "textColor": "{colors.text}"
    "rounded": "{rounded.dialog}"
    "padding": "23px"
    "width": "460px"
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
- 手机控件保持可触达，完整信息通过详情展开。

当前规范从 `src/styles.css` 与 `src/App.tsx` 提取；现有桌面、手机、选择、登录和传输详情截图位于 `.impeccable/review/`。

## Colors

冷白与灰蓝构成安静背景，操作蓝提供明确的行动信号。前面的 token 为规范值，sidecar 中的色阶仅用于面板预览。

### Primary

- **操作蓝**（`accent`）：上传、确认、保存和连接等主要操作，以及选择数量和焦点。
- **深操作蓝**（`accent-hover`）：可用主要按钮的悬停反馈。
- **浅导航蓝 / 选中蓝**（`nav-active`、`row-selected`、`row-focused`）：当前导航、文件选择与详情关联行。
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

**The Readable Metadata Rule.** 辅助文字、占位文字和状态文字保持清晰；普通正文与其背景的对比度至少为 4.5:1。

## Layout

工作区采用可伸缩的主区域，文件与传输列表在各自内容区滚动。共享间距集中在 4、6、8、10、12、16、20、24、32px；具体页面使用实际布局尺寸。

- **桌面：** 侧栏宽 228px，顶部条高 68px，工作区内边距为 32px 32px 22px。文件表以名称列吸收剩余空间，文件大小和修改时间按列对齐。详情侧栏默认为 260px。
- **宽屏（至少 1600px）：** 工作区内边距为 38px 44px 28px，详情侧栏为 290px。
- **紧凑桌面（至多 1150px）：** 侧栏收为 205px，工作区内边距为 25px 24px 18px；详情展开时隐藏修改时间列。
- **中间宽度（至多 900px）：** 账号页保持单列任务布局，文件行的更多操作持续可见。
- **手机（至多 700px）：** 侧栏隐藏，顶部条为 58px 加顶部安全区，主区内边距为 18px 17px 15px。底部导航为 65px 加底部安全区，内容和详情面板为它预留空间。
- **手机文件表：** 名称与大小保留，修改时间列隐藏；表格横向贴近屏幕边缘。工具栏可换行，搜索占独立整行。
- **列表密度：** 文件默认行高 64px，紧凑模式为 48px；传输列表按 120px 行高组织。选择状态在原工具栏位置显露批量操作。
- **触达：** 手机图标按钮、选择区域、主要表单字段、列表导航与操作按钮至少提供 48px 的交互尺寸；传输控制间距为 8px。

## Elevation & Depth

深度采用色面分层与细边界，常规文件行和面板保持平稳。对话框与提示浮在内容之上，背景遮罩使任务焦点清楚。

### Shadow Vocabulary

- **Dialog:** `0 18px 70px #14294435`，用于操作、传输详情和预览对话框。
- **Toast:** `0 8px 28px #18274024`，用于临时结果提示。
- **Backdrop:** `#263c5d55`，用于原生对话框后方。

**The Quiet Surface Rule.** 文件行、登录面板和设置面板以色面和边界分层；阴影集中于对话框与提示。

状态色变化通常持续 0.16s，文件行反馈持续 0.12s，开关持续 0.18s。手机详情采用 0.18s 的轻度横向进入。读取旋转与骨架脉冲表达进行状态；减少动态效果时关闭动画和过渡。

## Shapes

形状使用轻度圆角与细边界。短标签采用 `tag`，表单采用 `field`，常规按钮采用 `control`，文件表采用 `table`，登录和设置容器采用 `panel`，对话框采用 `dialog`。头像和连接提示保留圆形形态。

面板与字段使用单像素边界；手机文件表减去左右外框，并维持内部行分隔。文件类型使用小型轮廓图标，文件夹带有暖黄色填充。图标与文字承担工作界面的层级。

## Components

### Buttons

明确、紧凑的操作控件。主要、次要、安静和删除确认是四个实际变体。

- **Primary:** 操作蓝底与白字，可用状态悬停时切换深操作蓝。
- **Secondary:** 白底、灰蓝文字和细边界；悬停时轻度改变底色与边界。
- **Quiet:** 透明底、灰蓝文字，适合批量工具栏与辅助操作；悬停使用淡色面。
- **Destructive:** 删除确认使用红底白字；删除入口在工具栏使用红色文字。
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

文件名、类型、大小与时间形成可扫描的行。名称过长时在列表内省略，详情允许完整名称换行。选择行使用浅蓝背景，详情关联行使用更轻的蓝色面；选择后，批量下载、移动、复制和删除在持续的工具栏位置出现。手机的选择区域和更多按钮保持完整触控尺寸。

### Transfer Ledger & Details

任务行保留名称、文字状态、细进度条、数量信息和恢复操作。点击任务名打开详情，将完整名称、状态、进度、本地路径和失败原因放在可滚动对话框内。失败区采用浅红面与深红文字；路径和失败原因保留换行、任意长字符串折行与选择复制。

### Operation Dialogs

对话框默认宽 460px，最大宽为视口减 28px，最大高为动态视口减 40px，并允许滚动。目标目录显示面包屑、文件夹列表与读取状态；目录切换隔离迟到响应。确认按钮在读取目录或操作进行中保持禁用，连按 Enter 仅启动一次操作。

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

账号统一从侧栏底部的会话管理进入，其下为设置。桌面侧栏提供文件、传输、分享三项工作导航并可收缩；手机使用同一导航的窄图标栏。

账号内容采用最大 720px 的单列工作面。当前连接首先展示账号摘要与空间状态，容量使用情况紧随其后；空间切换在独立表单中选择目标、填写群组 ID 或分享链接并确认连接。更换账号表单按需展开，未登录时直接显示。登录方式使用带标签的选择器，短信验证与高级字段逐步展开。

账号名称为 18px，任务标题为 17px；手机账号名称为 16px，页标题为 22px。分区用留白与细横线组织。手机控件保持至少 48px 高，表单单列排列；错误紧邻所属表单，成功连接后进入文件列表。


## Navigation and appearance (0.1.3)

The desktop sidebar is 218 px wide, collapses to a 68 px rail, and keeps session management above settings at the bottom. Phones use a 56 px rail with 48 px controls. File content starts directly in the working area. Account identities appear once in the saved session list, with explicit connect, disconnect, and credential removal actions.

Light mode preserves the existing white workspace and slate navigation. Dark mode uses #141e2e working surfaces, #e3eaf5 primary text, #b4c1d5 secondary text, #33415a seams, #83adff links, and #306adc primary buttons with white labels. System mode responds to OS changes; desktop window chrome follows the chosen theme. Preferences persist locally.

Upload policy lives in a flat row above the transfer filters, using the same blue selected buttons, typography, and spacing. On phones the label and three equal-width 48 px controls stack vertically. It is chosen for a batch or an active account session. The persistent default lives in transfer settings. Provider constraints are visible: personal-space upload requires SHA256; the group streaming path can skip the preliminary hash scan.

Directory updates preserve the loaded working set and pagination during automatic synchronization. Completed uploads are published together in 150 ms batches, then reconciled within a fixed 1.5 s window; an in-flight listing finishes before another automatic request starts. Successful deletion results update the rows immediately, while errors remain actionable on the failed items.
