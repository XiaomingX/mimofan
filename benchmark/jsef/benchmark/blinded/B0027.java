package blinded;

import java.io.IOException;
import java.util.Arrays;
import java.util.List;









public class TaintSingleHopBy {

    // 白名单：仅允许这些 localhost 演示命令
    private static final List<String> ALLOWLIST = Arrays.asList("echo", "ping", "hostname");

    




    public void runCommand(String userInput) throws IOException {
        String[] parts = userInput.split("\\s+");
        if (parts.length == 0 || !ALLOWLIST.contains(parts[0])) {
            throw new IllegalArgumentException("command not allowed: " + parts[0]);
        }
        /*ANCHOR_1*/
        Process p = Runtime.getRuntime().exec(new String[]{"echo", "localhost-demo"});
    }

    public static void main(String[] args) throws IOException {
        new TaintSingleHopBy().runCommand("echo hello-localhost");
    }
}
