package blinded;

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
public class JdbcUrlInitBy {

    
    static final String MANAGED_URL = "jdbc:h2:mem:app;DB_CLOSE_DELAY=-1";

    
    static final String[] BLOCKED_PARAMS = {"INIT=", "RUNSCRIPT", "SCRIPT", "autoDeserialize", "allowLoadLocalInfile"};

    
    static final String[] ALLOWED_HOSTS = {"localhost", "127.0.0.1"};

    public Connection connect(String userInput) throws Exception {
        // ① 优先路径：固定受管 DataSource
        String url = MANAGED_URL;
        if (userInput != null && !userInput.isEmpty()) {
            // ② 用户片段经黑名单 + URI 校验后才可进入 URL
            url = validatedUrl(userInput);
        }
        /*ANCHOR_1*/
        return DriverManager.getConnection(url, "sa", "");
    }

    
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
        new JdbcUrlInitBy().connect(";INIT=RUNSCRIPT FROM 'http://localhost/evil.sql'");
    }
}
