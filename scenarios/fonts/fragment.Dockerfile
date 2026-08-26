# >>> scenario: fonts >>>
# L1 示例:字体层。base 镜像原本是"零字体"状态(连 fontconfig 都没有),
# 容器内任何服务端文本渲染(matplotlib 出图、pandoc/HTML→PDF、无头截图)
# 中文一律豆腐块。
#   - Maple Mono NF CN 一个家族同时覆盖:等宽 Latin + Nerd Font 图标 +
#     常用中文字形,正好匹配终端/代码场景。
#   - 装系统路径 /usr/local/share/fonts(不被 aio_workspace 卷遮盖)。
#   - 只取 4 个核心字重(Regular/Bold/Italic/BoldItalic,约 78MB):终端与
#     编辑器实际只用这四个;上游全量 16 字重 +319MB 不划算,需要时自行扩
#     unzip 的文件列表。
#   - 注意:code-server / 工作台网页在宿主浏览器渲染,字体栈取宿主系统字体
#     (web/src/styles.css 的 --font-mono),本场景不改变网页显示字体。
# apt lists 在 head 末尾被 `rm -rf /var/lib/apt/lists/*` 清空,这里需重新 update。
ARG MAPLE_VERSION=7.9
ARG MAPLE_NF_CN_SHA256=af913b6322905348b3f50e4397fedc35b3a880db5effcce7969003051dcd3e94
RUN apt-get update \
 && apt-get install -y --no-install-recommends fontconfig unzip \
 && rm -rf /var/lib/apt/lists/* \
 && curl -fsSL -o /tmp/MapleMono-NF-CN.zip \
      https://github.com/subframe7536/Maple-font/releases/download/v${MAPLE_VERSION}/MapleMono-NF-CN.zip \
 && echo "${MAPLE_NF_CN_SHA256}  /tmp/MapleMono-NF-CN.zip" | sha256sum -c - \
 && mkdir -p /usr/local/share/fonts/maple-mono-nf-cn \
 && unzip -j -o /tmp/MapleMono-NF-CN.zip \
      MapleMono-NF-CN-Regular.ttf MapleMono-NF-CN-Bold.ttf \
      MapleMono-NF-CN-Italic.ttf MapleMono-NF-CN-BoldItalic.ttf \
      LICENSE.txt \
      -d /usr/local/share/fonts/maple-mono-nf-cn \
 && rm -f /tmp/MapleMono-NF-CN.zip \
# fontconfig 会随包带上 fonts-dejavu-core,且内置通用族排序 DejaVu 优先(实测
# fc-match monospace/sans-serif:lang=zh 都落到无中文的 DejaVu)。写
# /etc/fonts/local.conf 用 strong alias 把三个通用族钉到 Maple(容器里唯一带
# 中文字形的字体),DejaVu 自动退居 glyph 兜底。
 && printf '%s\n' \
      '<?xml version="1.0"?>' \
      '<!DOCTYPE fontconfig SYSTEM "fonts.dtd">' \
      '<fontconfig>' \
      '  <alias binding="strong"><family>monospace</family><prefer><family>Maple Mono NF CN</family></prefer></alias>' \
      '  <alias binding="strong"><family>sans-serif</family><prefer><family>Maple Mono NF CN</family></prefer></alias>' \
      '  <alias binding="strong"><family>serif</family><prefer><family>Maple Mono NF CN</family></prefer></alias>' \
      '</fontconfig>' > /etc/fonts/local.conf \
 && fc-cache -f \
# 断言(不是打印):通用族必须解析到 Maple,且 cmap 里真有中文与 NF 图标字形,
# 任何一条失败构建即失败。
 && fc-match -f '%{family}\n' monospace | grep -qi maple \
 && fc-match -f '%{family}\n' 'sans-serif:lang=zh' | grep -qi maple \
 && fc-list ':charset=4e2d' family | grep -qi maple \
 && fc-list ':charset=e0b0' family | grep -qi maple
# <<< scenario: fonts <<<
