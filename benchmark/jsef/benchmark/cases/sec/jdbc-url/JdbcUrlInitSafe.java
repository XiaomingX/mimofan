package com.jsef.benchmark.sec.jdbcurl;

import java.net.URI;
import java.sql.Connection;
import java.sql.DriverManager;
import java.util.Locale;

/*
 * JSEF-Benchmark L3 — JDBC URL 注入修复（CWE-94）
 *
 * 修复：
 *   ① 默认使用应用受管的固定 DataSource URL，不接受用户完整 URL；
 *   ② 即使传入用户片段，也先拒绝 INIT/RUNSCRIPT/autoDeserialize 等危险参数，
 *      再经 java.net.URI 解析并校验仅允许 localhost 的 mem: 连接。
 *
 * CWE-94 (Improper Control of Generation of Code)。
 */
public class JdbcUrlInitSafe {

    /** 连接池由应用受管，URL 固定，用户不可直接控制。 */
    static final String MANAGED_URL = "jdbc:h2:mem:app;DB_CLOSE_DELAY=-1";

    /** 拒绝出现的危险 JDBC URL 参数（大小写不敏感）。 */
    static final String[] BLOCKED_PARAMS = {"INIT=", "RUNSCRIPT", "SCRIPT", "autoDeserialize", "allowLoadLocalInfile"};

    /** 仅允许 localhost 演示的主机集合。 */
    static final String[] ALLOWED_HOSTS = {"localhost", "127.0.0.1"};

    public Connection connect(String userInput) throws Exception {
        // ① 优先路径：固定受管 DataSource
        String url = MANAGED_URL;
        if (userInput != null && !userInput.isEmpty()) {
            // ② 用户片段经黑名单 + URI 校验后才可进入 URL
            url = validatedUrl(userInput);
        }
        // [CHECKPOINT id=JSEF-JDBCURL-001S cwe=94 level=L3 source=user-controlled jdbc url sink=DriverManager.getConnection(blocklisted+URI-validated url) expect=SAFE]
        return DriverManager.getConnection(url, "sa", "");
    }

    /** 校验用户片段：拒绝危险参数，URI 解析后仅放行 localhost 的 mem: URL。 */
    static String validatedUrl(String userInput) throws Exception {
        String lower = userInput.toLowerCase(Locale.ROOT);
        for (String bad : BLOCKED_PARAMS) {
            if (lower.contains(bad.toLowerCase(Locale.ROOT))) {
                throw new IllegalArgumentException("blocked jdbc url fragment: " + bad);
            }
        }
        URI uri = new URI(userInput); // 语义等价：规范化 + host 白名单校验
        String host = uri.getHost() == null ? "localhost" : uri.getHost();
        boolean allowed = false;
        for (String h : ALLOWED_HOSTS) {
            if (h.equals(host)) {
                allowed = true;
                break;
            }
        }
        if (!allowed) {
            throw new IllegalArgumentException("non-localhost jdbc url: " + host);
        }
        return "jdbc:h2:mem:" + host;
    }

    public static void main(String[] args) throws Exception {
        new JdbcUrlInitSafe().connect(";INIT=RUNSCRIPT FROM 'http://localhost/evil.sql'");
    }
}
