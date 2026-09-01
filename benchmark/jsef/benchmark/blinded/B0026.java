package blinded;

import java.io.IOException;
import java.util.Arrays;
import java.util.List;










public class TaintMultiHopBy {

    private static final List<String> ALLOWLIST = Arrays.asList("echo", "ping", "hostname");

    public void runCommand(String userInput) throws IOException {
        String a = userInput;
        String b = a + " ; echo localhost";
        String c = b.trim();
        // 净化：解析命令名并校验白名单，仅用常量命令字面值
        String name = c.split("\\s+")[0];
        if (!ALLOWLIST.contains(name)) {
            throw new IllegalArgumentException("command not allowed: " + name);
        }
        /*ANCHOR_1*/
        Process p = Runtime.getRuntime().exec(new String[]{"echo", "localhost-demo"});
    }

    public static void main(String[] args) throws IOException {
        new TaintMultiHopBy().runCommand("echo hi");
    }
}
