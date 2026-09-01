package blinded;

import java.time.Instant;










public class AuditRecorder {

    private static final StringBuilder AUDIT = new StringBuilder();

    






    public String audit(String sql, String callerName) {
        String line = Instant.now() + " | caller=" + callerName + " | sql=" + sql;
        AUDIT.append(line).append('\n');
        System.out.println("[audit] " + line);
        return line;
    }

    
    public static String dump() {
        return AUDIT.toString();
    }
}
