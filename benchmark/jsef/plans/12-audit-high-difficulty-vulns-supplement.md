# JSEF × 审计机构高危漏洞清单 样本补充计划

> 目标：对照审计机构提供的 **SECURITY_TODO_LIST（34 项高检测难度漏洞）**，逐项核查 JSEF 现有样本覆盖，对 **PARTIAL / MISSING** 项补充 `vuln + safe` 对照样本，同步更新 `expectedresults.csv`（唯一注册表）；按需评估脚本增强。
>
> 全程遵循 `AGENTS.md` / `CLAUDE.md` 双源门禁：`// [CHECKPOINT]` 注解 + `expectedresults.csv` 一致、`validate_checkpoints.py` 退出码 0、payload 仅 localhost 语义、不写真实攻击脚本。

---

## 0. 审计结论（34 项覆盖矩阵，已由子代理逐项核实源码 + CSV）

> 判定依据：`benchmark/expectedresults.csv`（808 行）的 sink/category 列 + `benchmark/cases/{vuln,sec}` 与 `src/.../vulnerability` 源码语义。EXISTS=已有完整 vuln+safe；PARTIAL=有近义变体但缺清单指定语义；MISSING=无对应样本。

### EXISTS（10 项）—— 已覆盖，不新增
| # | 项 | 证据（现有样本） |
|---|---|---|
| A1 | 二阶 SQL 注入 | `vuln/taint-variants/StoredTaintSql.java` + Safe（JSEF-TV-001） |
| A7 | 反序列化白名单时序错误 | `vuln/xstream-late/XstreamLateAllowlistVuln.java` + Safe（allowlist AFTER parse） |
| A9 | TOCTOU 文件检查竞争 | `vuln/temp-file/InsecureTempFileRace.java` + Safe（JSEF-QL-004） |
| B2 | SSRF 重定向跟随 | `vuln/ssrf-redirect/SsrfRedirectVuln.java` + Safe（JSEF-NV506） |
| B3 | SSRF 非 HTTP scheme | `vuln/ssrf-metadata/SsrfSchemeMetadataVuln.java`（JSEF-SSM-001） |
| B9 | ORDER BY 注入 | `vuln/sql/mybatis/MybatisMapperInjection.java` `findByOrder(${})` + Safe（JSEF-SQL-001） |
| B10 | MongoDB `$where` 注入 | `vuln/NosqlInjectionMongo.java`（JSEF-NOSQL-001） |
| B12 | `sh -c` 数组执行陷阱 | `vuln/confusion/CmdPartialAllowlist.java` + Safe（JSEF-PF-002） |
| C1 | AES-GCM 固定 IV / nonce 复用 | `vuln/crypto/ReusedIv.java` + Safe（JSEF-A02-002） |
| C3 | 异常控制流污点 | `vuln/exception-path-exec/ExceptionExecVuln.java` + Safe（JSEF-NV503） |

### PARTIAL（10 项）—— 已有近义变体，各补一个"清单指定语义"新样本（不动旧样本，避免行号连坐）
| # | 项 | 现有近义样本 | 缺的具体语义 |
|---|---|---|---|
| A3 | JDBC URL 攻击 | `vuln/level5/GadgetChainJdbc.java`（autoDeserialize） | H2 `;INIT=RUNSCRIPT` 直递拼接 |
| A8 | 名存实亡 JEP290 过滤器 | `level0/L0DeserDirect*`（无 filter / 正常 allowlist） | filter 已装但只打日志/判非 null 即放行 |
| B1 | URL 解析器微分 SSRF | `vuln/taint-variants/PrefixCheckSsrf.java`（startsWith 弱校验） | userinfo `@` / 反斜杠 / fragment 绕过 vs `URI.getHost` |
| B6 | Thymeleaf 片段视图名 SSTI | `vuln/thymeleaf/ThymeleafInjection.java`（view name 注入） | `"page :: " + section` 片段拼接求值 |
| B8 | JWT jku 白名单后缀绕过 | `vuln/jwt-jku/JwtJkuVuln.java`（任意 jku 拉取） | `startsWith(trustDomain)` 被 `.evil.com` 后缀绕过 |
| C4 | Stream 消毒结果被丢弃 | `vuln/taint-variants/DiscardedSanitizeVuln.java`（SQL 版） | `.map(this::sanitize)` 未 collect + `String.join` 原脏 list → exec |
| C5 | 整数溢出致分配失控 | `vuln/int-overflow/QtyOverflowVuln.java`（金额版） | `size*4` 溢出 → 巨分配 / 越界 / DoS |
| C6 | 信任 XFF 做鉴权 | `src/.../businessLogic/IpSpoofingVulnerabilityController.java`（仅教学、未登记 CSV、展示型） | 伪造头判内网/白名单 → 授权放行（登记 CSV） |
| C11 | Tar Slip | `vuln/zip-slip/ZipSlip.java`（ZIP 版） | `TarArchiveInputStream` 解压路径穿越 |
| C12 | 深层嵌套 JSON/XML DoS | `vuln/xml-dos/XmlBombVuln.java`（DTD 实体展开 CWE-776） | 递归深度无限制 → 栈溢出（CWE-400） |

### MISSING（14 项）—— 全新增
A2 SAML XSW · A4 Web Cache Deception · A5 Host 头投毒→密码重置 · A6 WebSocket 缺鉴权 · B4 安全过滤器路径匹配绕过（矩阵/分号） · B5 授权通配过浅（AntPathMatcher） · B7 Pebble SSTI · B11 Unicode NFKC 顺序 · B13 正则 Matcher.find 未锚定 · C2 Padding Oracle · C7 SimpleDateFormat 并发 · C8 CSV 公式注入 · C9 SMTP 邮件头注入 · C10 XSS 上下文错配

---

## 1. 设计原则

1. **新样本一律放新 category 目录 + 全新 id 前缀**（下表前缀已核对 `expectedresults.csv` 全部现有前缀，无冲突）；safe 样本 id = vuln id + 后缀 `S`，文件名 `XxxVuln.java` ↔ `XxxSafe.java`。
2. **静态可读、不编译**（与 `benchmark/cases` 惯例一致，CLAUDE.md「不要求编译」）；第三方类用真实 import（`org.opensaml.*`、`javax.websocket.*`、`com.mitchellbosecke.pebble.*` 等）+ 桩/注释声明语义，符合 AGENTS.md 桩约定。
3. **trace= 仅用于 L3+ 跨节点样本**；每节点 `file:line` 必须指向真实源码行，含逗号时 CSV 用双引号包裹整列。
4. **不动 `scorecard.py` / `validate_checkpoints.py`**（实证：`c8057b2`、`82d7fbd` 新增样本均未改脚本）；唯一注册表是 `expectedresults.csv`。Phase 4 的脚本增强按"先审计再启用"顺序、可选。
5. **每个 vuln 必配 safe**，safe 实现真实防护（参数化 / 白名单 / 原子操作 / 归一化后校验等），用于 FP/TN 区分。
6. **每阶段结束必跑双源校验**：`validate_checkpoints.py` 退出码必须 0。

---

## 2. 实施阶段

> 每对样本的完成定义（AGENTS.md 门禁）：① 在**污点到达 sink 的精确行**上方加 `// [CHECKPOINT ...]`；② 追加 10 列 CSV 行（`type` 与 `expect` 一致、`line` = 注解实际行号、L3+ 跨节点填 `trace`）；③ `validate_checkpoints.py` 退出码 0。

### Phase 1 — A 类：长程记忆 / 跨阶段 / 多步骤（6 对）

#### A2 SAML XML 签名包裹 XSW
- 落点：`benchmark/cases/vuln/saml-xsw/SamlXswVuln.java` + `sec/saml-xsw/SamlXswSafe.java`
- CHECKPOINT：`id=JSEF-SAML-001 cwe=347 level=L5 source=attacker-controlled SAML response sink=authorization based on unsigned injected assertion expect=VULN`（safe 用 `...S` / `expect=SAFE`，sink=authorization based on signature-covered assertion）
- 语义要点：验签在"被签文档（SignedInfo 引用的原始 `<Assertion>`）"上通过 → 鉴权却 `getElementsByTagName("Assertion")` 取**攻击者包裹注入的未签名副本**（XML Signature Wrapping，包一层新 `<Assertion>`）→ 按注入断言里的 role 放行。类：`XMLSignature.validate()` + DOM 遍历。
- trace：3 节点（验签行 → 读断言行 → 授权行），同文件或拆 `SamlXswParser` 辅助类。

#### A3 JDBC URL 攻击（H2 INIT=RUNSCRIPT）
- 落点：`vuln/jdbc-url/JdbcUrlInitVuln.java` + `sec/jdbc-url/JdbcUrlInitSafe.java`
- CHECKPOINT：`id=JSEF-JDBCURL-001 cwe=94 level=L3 source=user-controlled jdbc url sink=DriverManager.getConnection expect=VULN`
- 语义要点：`String url = "jdbc:h2:mem:db;INIT=RUNSCRIPT FROM '" + userInput + "'"` → 用户 URL 注入任意 SQL 脚本（含 `http://attacker/x.sql`）。safe：固定受管 DataSource、拒绝 `INIT`/`SCRIPT` 参数、URL 白名单解析。
- trace：3 节点（拼接 URL → getConnection → 脚本执行注释声明）。

#### A4 Web Cache Deception
- 落点：`vuln/cache-deception/WebCacheDeceptionVuln.java` + `sec/cache-deception/WebCacheDeceptionSafe.java`
- CHECKPOINT：`id=JSEF-WCD-001 cwe=285 level=L4 source=request path with .css suffix sink=sensitive response body cached expect=VULN`
- 语义要点：鉴权页 `/account` 返回敏感体；过滤器按后缀 `.css`（或分号段）放行 → 路由归一化后仍执行动态 Controller 返回 200 敏感内容 → 注释声明"反向代理按 URL 后缀静态缓存、缓存键不含鉴权态"，导致公网可缓存访问。safe：`Cache-Control: private, no-store` + 严格路径校验（拒绝动态路由伪装静态后缀）。
- trace：4 节点（过滤器放行 → 归一化 → Controller 响应 → 缓存键声明）。

#### A5 Host 头投毒 → 密码重置链接
- 落点：`vuln/host-header-reset/HostHeaderResetVuln.java` + `sec/host-header-reset/HostHeaderResetSafe.java`
- CHECKPOINT：`id=JSEF-HOSTRESET-001 cwe=601 level=L3 source=Host header sink=reset link base from attacker-controlled Host expect=VULN`
- 语义要点：`String base = "https://" + request.getHeader("Host")` → `resetLink = base + "/reset?token=" + token` 经 `Transport.send` 邮件发出 → 攻击者设 `Host: evil.com` 即劫持重置链接。safe：固定配置 base URL + Host 白名单校验 + `URI` 严格解析。
- trace：3 节点（读 Host → 拼 link → 邮件发送）。

#### A6 WebSocket 缺鉴权
- 落点：`vuln/websocket-authz/WebSocketNoAuthzVuln.java` + `sec/websocket-authz/WebSocketNoAuthzSafe.java`
- CHECKPOINT：`id=JSEF-WS-001 cwe=862 level=L3 source=websocket message expect=... sink=message handler without session identity check expect=VULN`
- 语义要点：HTTP 握手端做一次鉴权存入 session，`@ServerEndpoint` 的 `onOpen`/`onMessage` 不再校验（或只信任握手期 user id），消息处理器直接按消息内携带的 user 操作敏感资源。safe：`onOpen` 校验 + 消息处理器每次 `session.getUserPrincipal()` 复核。
- trace：3 节点（握手鉴权 → onOpen 存身份 → onMessage 未复核）。

#### A8 名存实亡的 JEP290 过滤器
- 落点：`vuln/jep290-dead/Jep290DeadFilterVuln.java` + `sec/jep290-dead/Jep290DeadFilterSafe.java`
- CHECKPOINT：`id=JSEF-JEP290-001 cwe=502 level=L3 source=serialized payload sink=ObjectInputFilter only logs / returns UNDECIDED then readObject expect=VULN`
- 语义要点：`ois.setObjectInputFilter(info -> { logger.info(...); return UNDECIDED; })`（或 `filter != null` 即放行）→ gadget 照常触发；注释强调"有过滤器"是**假防护**。safe：真实 allowlist（危险包 `REJECTED`、其余 `UNDECIDED`/`REJECTED` 白名单）。
- trace：3 节点（set filter → filter 返回 UNDECIDED → readObject 触发 gadget）。

### Phase 2 — B 类：解析器微分 / 框架状态机 / 复杂语义（8 对）

#### B1 URL 解析器微分 SSRF
- 落点：`vuln/url-confusion/UrlParserConfusionVuln.java` + `sec/url-confusion/UrlParserConfusionSafe.java`
- CHECKPOINT：`id=JSEF-URLCONF-001 cwe=918 level=L4 source=attacker url sink=URL.openConnection after string prefix check (userinfo/@/backslash bypass) expect=VULN`
- 语义要点：`url.startsWith("https://trusted.com/")` 通过后 `new URL(url).openConnection()`；`https://trusted.com@evil.com/`（userinfo）、`https://trusted.com.evil.com/`、反斜杠 `https://trusted.com\@evil.com/`、`#fragment` 均可让解析后的 host ≠ 字符串前缀。safe：用 `java.net.URI.getHost()` 精确 host 比较 + scheme 白名单 + 禁止 userinfo。
- trace：3 节点（字符串校验 → URL 解析 → openConnection）。

#### B4 安全过滤器路径匹配绕过（矩阵变量 / 分号段）
- 落点：`vuln/matrix-path-bypass/MatrixPathAuthzVuln.java` + `sec/matrix-path-bypass/MatrixPathAuthzSafe.java`
- CHECKPOINT：`id=JSEF-MATRIX-001 cwe=863 level=L3 source=/admin;x.css style path sink=authorization decision after suffix-match skip expect=VULN`
- 语义要点：安全过滤器 `path.endsWith(".css")` 即跳过鉴权；Spring 控制器侧路由归一化剥掉 `;x.css` → `/admin` 命中管理员接口（分号矩阵变量段）。safe：鉴权基于归一化后路径（`UrlPathHelper`/`PathContainer` 去分号段）且精确匹配白名单。
- trace：3 节点（后缀匹配跳鉴权 → 分号段归一化 → admin 放行）。

#### B5 授权通配过浅（AntPathMatcher 深度语义）
- 落点：`vuln/ant-pattern-depth/AntPatternShallowVuln.java` + `sec/ant-pattern-depth/AntPatternShallowSafe.java`
- CHECKPOINT：`id=JSEF-ANTPAT-001 cwe=863 level=L3 source=request URI /admin/report/export sink=shallow wildcard /admin/* authorizes too little expect=VULN`
- 语义要点：`authorize("/admin/*")`（AntPathMatcher 单段通配）不覆盖 `/admin/report/export` → 深层路径免鉴权直通；注释解释 `/*` 与 `/**` 段数语义。safe：`/admin/**` 或精确路径集。
- trace：3 节点（匹配规则 → 未命中 → 放行敏感操作）。

#### B6 Thymeleaf 片段视图名 SSTI
- 落点：`vuln/thymeleaf-fragment/ThymeleafFragmentVuln.java` + `sec/thymeleaf-fragment/ThymeleafFragmentSafe.java`
- CHECKPOINT：`id=JSEF-THYFRAG-001 cwe=1336 level=L3 source=user section fragment sink=TemplateEngine.process("page :: "+section) expression eval expect=VULN`
- 语义要点：`viewName = "page :: " + section` → ViewResolver 遇到 `::` 把 `section` 当**表达式**解析（SpEL/OGNL 求值）→ SSTI。safe：section 白名单固定集、禁止 `::` 前缀拼接。
- trace：3 节点（拼 viewName → 解析片段 → 表达式求值）。

#### B7 Pebble SSTI
- 落点：`vuln/pebble-ssti/PebbleSstiVuln.java` + `sec/pebble-ssti/PebbleSstiSafe.java`
- CHECKPOINT：`id=JSEF-PEBBLE-001 cwe=1336 level=L2 source=user input in inline template sink=PebbleEngine.getLiteralTemplate(eval).evaluate expect=VULN`
- 语义要点：`engine.getLiteralTemplate("Hello " + userInput).evaluate(writer, ctx)` → 用户输入内联进模板源被求值。safe：固定模板 + 用户输入仅作上下文变量。
- trace：无（L2 单点，可留空）。

#### B8 JWT jku 白名单后缀绕过
- 落点：`vuln/jwt-jku-suffix/JwtJkuSuffixVuln.java` + `sec/jwt-jku-suffix/JwtJkuSuffixSafe.java`
- CHECKPOINT：`id=JSEF-JKUSFX-001 cwe=345 level=L3 source=jku header startsWith bypass sink=JWT verify using attacker JWKS (trust.issuer.com.evil.com) expect=VULN`
- 语义要点：`if (jkuHost.startsWith(TRUSTED))` 信任域校验 → `https://trust.issuer.com.evil.com/key.json` 通过 → 拉攻击者公钥验签伪造 token。safe：`URI.getHost()` + 精确相等 / `endsWith(".trust.issuer.com")` 边界匹配 + kid 本地钉扎。
- trace：3 节点（startsWith 校验 → 拉 JWKS → 验签放行）。

#### B11 Unicode 规范化顺序
- 落点：`vuln/unicode-nfkc/UnicodeNormalizeOrderVuln.java` + `sec/unicode-nfkc/UnicodeNormalizeOrderSafe.java`
- CHECKPOINT：`id=JSEF-NFKC-001 cwe=176 level=L3 source=input with fullwidth ＠/= sink=normalize(NFKC) AFTER validation revives taint expect=VULN`
- 语义要点：先 `input.contains("@")` 校验拦截 → 再 `Normalizer.normalize(input, NFKC)` → 全角 `＠`/`＝` 归一成 `@`/`=` 复活绕过（SSRF 或凭据注入）。safe：**先 NFKC 归一化再校验**。
- trace：3 节点（校验 → 归一化 → 复活 sink）。

#### B13 正则 Matcher.find 未锚定
- 落点：`vuln/regex-unanchored/MatcherFindUnanchoredVuln.java` + `sec/regex-unanchored/MatcherFindUnanchoredSafe.java`
- CHECKPOINT：`id=JSEF-UNANCHORED-001 cwe=185 level=L2 source=attacker url sink=Pattern.find() substring match allows expect=VULN`
- 语义要点：`SAFE_PATTERN.matcher(url).find()` 子串命中即放行 → `https://example.com.evil.com` 含前缀子串 `https://example.com` 通过白名单。safe：`.matches()` 全匹配锚定（或 `\A...\z`）。
- trace：无（L2）。

### Phase 3 — C 类：时序 / 控制流 / 假防护 / 加密（10 对）

#### C2 Padding Oracle
- 落点：`vuln/padding-oracle/PaddingOracleVuln.java` + `sec/padding-oracle/PaddingOracleSafe.java`
- CHECKPOINT：`id=JSEF-PADORACLE-001 cwe=327 level=L3 source=ciphertext expect=... sink=CBC decrypt with distinguishable padding error responses expect=VULN`
- 语义要点：AES/CBC/PKCS5Padding，解密/反填充异常被上层 catch 成**可区分响应**（如 400 vs 500）→ 逐字节解密/伪造（注释声明 oracle 语义）。safe：GCM 认证加密 + 恒时错误 + 不向调用方区分 padding 错误。
- trace：3 节点（CBC 解密 → 异常分支 → 区分响应）。

#### C4 Stream 消毒结果被丢弃（command 变体）
- 落点：`vuln/stream-sanitize-drop/StreamSanitizeDropVuln.java` + `sec/stream-sanitize-drop/StreamSanitizeDropSafe.java`
- CHECKPOINT：`id=JSEF-STREAMSAN-001 cwe=78 level=L3 source=user command args sink=Runtime.exec(String.join after discarded map sanitize) expect=VULN`
- 语义要点：`list.stream().map(this::sanitize);`（**未 collect**）→ `String.join(" ", list)`（原脏 list）→ `exec`；见到"调用了 sanitize"即误判安全。safe：`list = list.stream().map(this::sanitize).collect(toList())` 再 join。
- trace：3 节点（map 丢弃 → join 原 list → exec）。

#### C5 整数溢出致分配失控
- 落点：`vuln/alloc-overflow/AllocSizeOverflowVuln.java` + `sec/alloc-overflow/AllocSizeOverflowSafe.java`
- CHECKPOINT：`id=JSEF-ALLOC-001 cwe=190 level=L3 source=size param sink=new byte[size*4] overflow → giant allocation/DoS expect=VULN`
- 语义要点：`int size = Integer.parseInt(lenParam); byte[] buf = new byte[size * 4];` → 乘法溢出为负（NegativeArraySizeException DoS）或回绕巨值（OOM）。safe：`long` 运算 + 上限校验。
- trace：无（L3 但同文件直线，可留空）。

#### C6 信任 X-Forwarded-For 做鉴权
- 落点：`vuln/xff-authz/XForwardedForAuthzVuln.java` + `sec/xff-authz/XForwardedForAuthzSafe.java`
- CHECKPOINT：`id=JSEF-XFF-001 cwe=290 level=L2 source=X-Forwarded-For header sink=authorization decision based on spoofable header expect=VULN`
- 语义要点：`String ip = request.getHeader("X-Forwarded-For"); if (trustedIps.contains(ip)) admin();` → 客户端直接设头伪造内网 IP。safe：`request.getRemoteAddr()` + 受信代理链（仅解析可信代理追加段）+ 不在业务层信任 XFF。
- trace：无（L2）。

#### C7 共享非线程安全 SimpleDateFormat
- 落点：`vuln/simple-date-format/SharedSimpleDateFormatVuln.java` + `sec/simple-date-format/SharedSimpleDateFormatSafe.java`
- CHECKPOINT：`id=JSEF-SDF-001 cwe=567 level=L3 source=concurrent parse of token expiry sink=shared SimpleDateFormat corrupted date comparison expect=VULN`
- 语义要点：单例 `SimpleDateFormat fmt` 多线程 `parse` token 过期时间 → 并发污染解析结果 → 过期 token 误判有效。safe：`ThreadLocal<SimpleDateFormat>` 或 `java.time` 不可变类。
- trace：3 节点（共享字段 → 并发 parse → 过期判断）。

#### C8 CSV 公式注入
- 落点：`vuln/csv-formula/CsvFormulaInjectionVuln.java` + `sec/csv-formula/CsvFormulaInjectionSafe.java`
- CHECKPOINT：`id=JSEF-CSVFORMULA-001 cwe=1236 level=L2 source=user cell value sink=CSV write with leading =/+/-/@/Tab/CR expect=VULN`
- 语义要点：用户值以 `=` `+` `-` `@` Tab CR 开头未中和即写入 CSV → Excel 公式执行（`=HYPERLINK`、`+cmd|...`）。safe：前导危险字符前缀 `'` 或整体拒绝/白名单。
- trace：无（L2）。

#### C9 SMTP / 邮件头注入
- 落点：`vuln/mail-header-injection/MailHeaderInjectionVuln.java` + `sec/mail-header-injection/MailHeaderInjectionSafe.java`
- CHECKPOINT：`id=JSEF-MAILINJ-001 cwe=93 level=L2 source=recipient/subject with CRLF sink=MailMessage.setSubject/setRecipients header injection expect=VULN`
- 语义要点：`msg.setSubject("Reset for " + userName)` 未剥 CR/LF → `\r\nBcc: attacker@evil.com` 注入密送/额外头。safe：CR/LF 剥离 + RFC 头字段校验 + 仅用受管收件人。
- trace：无（L2）。

#### C10 XSS 上下文错配
- 落点：`vuln/xss-context-mismatch/XssJsContextMismatchVuln.java` + `sec/xss-context-mismatch/XssJsContextMismatchSafe.java`
- CHECKPOINT：`id=JSEF-XSSCTX-001 cwe=79 level=L3 source=user name sink=script single-quote context after HtmlUtils.htmlEscape expect=VULN`
- 语义要点：`"var name = '" + HtmlUtils.htmlEscape(user) + "';"` 嵌入 `<script>` 单引号串 —— htmlEscape 不按 JS 上下文处理 `'`/`\`/`</script>` → 引号逃逸/闭合标签注入。safe：JS 专用转义（或避免拼接进 JS，用 textContent 输出）。
- trace：3 节点（escape → 拼进 script → 浏览器执行）。

#### C11 Tar Slip
- 落点：`vuln/tar-slip/TarSlipVuln.java` + `sec/tar-slip/TarSlipSafe.java`
- CHECKPOINT：`id=JSEF-TARSLIP-001 cwe=22 level=L2 source=tar entry name sink=TarArchiveInputStream entry path traversal write expect=VULN`
- 语义要点：`while ((e = tar.getNextTarEntry()) != null) Files.copy(tar, Paths.get(dest, e.getName()))` → `../` 穿越覆盖。safe：`e.getName()` 规范化 + 目标前缀校验 + 拒绝绝对路径/`..`。
- trace：无（L2）。

#### C12 深层嵌套 JSON / XML DoS
- 落点：`vuln/deep-nesting-dos/DeepNestingDosVuln.java` + `sec/deep-nesting-dos/DeepNestingDosSafe.java`
- CHECKPOINT：`id=JSEF-DEPTH-001 cwe=400 level=L2 source=deeply nested json/xml sink=recursive parse without depth limit → stack overflow expect=VULN`
- 语义要点：递归 JSON/XML 解析器无 `maxDepth` 开关 → 10 万层嵌套 → 栈溢出/CPU DoS。safe：深度计数器 + 上限拒绝。
- trace：无（L2）。

---

## 3. 验证清单（每阶段必做）

1. 双源校验（门禁，退出码必须 0，阶段**前后各跑一次**）：
   ```bash
   python3 benchmark/scripts/validate_checkpoints.py \
     --expected benchmark/expectedresults.csv \
     --cases-dir benchmark/cases \
     --src-dir src/main/java/com/freedom/securitysamples/vulnerability
   ```
2. 新 id 检查：`grep` 确认 `JSEF-{SAML,JDBCURL,WCD,HOSTRESET,WS,JEP290,URLCONF,MATRIX,ANTPAT,THYFRAG,PEBBLE,JKUSFX,NFKC,UNANCHORED,PADORACLE,STREAMSAN,ALLOC,XFF,SDF,CSVFORMULA,MAILINJ,XSSCTX,TARSLIP,DEPTH}` 前缀全局唯一、无孤儿/重复、`line` 与注解实际行号一致（漂移 0）。
3. CSV 10 列齐全；含逗号的 `trace` 字段用双引号包裹整列；`type` 与 `expect` 一致。
4. 可选自测：`python3 benchmark/scripts/scorecard.py --expected benchmark/expectedresults.csv --result <结果>` 验证新 id 可关联计分。
5. 抽查 2-3 个新样本人工复核污点流（source→sink）可读、safe 语义真实防护。

## 4. 反模式守卫

- **不改 `scorecard.py` / `validate_checkpoints.py`**（除非 Phase 4 已启用新校验）；不新增第三方依赖。
- **不编译 `benchmark/cases`** 样本（CLAUDE.md 明确不要求）；第三方类用 import + 桩 + 注释声明语义，不用真实可执行攻击载荷。
- **不改任何既有样本行 / 既有 CSV 行**（避免行号漂移与 id 连坐）；新增一律追加。
- **payload 仅 localhost 演示语义**，不写真实目标利用脚本。
- 目录名与 id 前缀**保持一个 category 一个前缀**，safe 恒用 `id + S`，不得复用他类前缀。

## 5. 交付产物汇总

- 24 个新 category 目录（`vuln` + `sec` 各 24 文件，共 48 个 `.java`）+ 24 对 CHECKPOINT 注解。
- `benchmark/expectedresults.csv` 追加 **48 行**（vuln 24 + safe 24）。
- Phase 4（可选）：`type↔expect` 一致性审计报告 + `validate_checkpoints.py` 增量校验。
- 本计划文件作为执行依据，供 `/claude-mem:do` 分阶段执行。
