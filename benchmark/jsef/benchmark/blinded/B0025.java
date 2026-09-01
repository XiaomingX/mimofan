package blinded;

import java.io.IOException;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;









public class TaintIndirectMapBy {

    private static final List<String> ALLOWLIST = Arrays.asList("echo", "ping", "hostname");

    public void runCommand(String userInput) throws IOException {
        Map<String, Object> ctx = new HashMap<>();
        ctx.put("cmd", userInput);

        Object field = ctx.get("cmd");
        String resolved = String.valueOf(field);

        String name = resolved.split("\\s+")[0];
        if (!ALLOWLIST.contains(name)) {
            throw new IllegalArgumentException("command not allowed: " + name);
        }
        /*ANCHOR_1*/
        Process p = Runtime.getRuntime().exec(new String[]{"echo", "localhost-demo"});
    }

    public static void main(String[] args) throws IOException {
        new TaintIndirectMapBy().runCommand("echo hi");
    }
}
