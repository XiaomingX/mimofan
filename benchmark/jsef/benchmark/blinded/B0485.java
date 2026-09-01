package blinded;

import java.time.Instant;










public class SpelSecurityLogger {

    private static final StringBuilder SEC_LOG = new StringBuilder();

    





    public String logExpression(String expression) {
        String stack = stackTrace();
        String line = Instant.now() + " | expr=" + expression + " | stack=" + stack;
        SEC_LOG.append(line).append('\n');
        System.out.println("[security-log] " + line);
        return line;
    }

    private String stackTrace() {
        StringBuilder sb = new StringBuilder();
        for (StackTraceElement el : Thread.currentThread().getStackTrace()) {
            sb.append(el.getClassName()).append('#').append(el.getMethodName()).append(';');
        }
        return sb.toString();
    }

    
    public static String dump() {
        return SEC_LOG.toString();
    }
}
