package blinded;

import java.util.Arrays;
import java.util.List;











public class ChainControllerBy {

    private static final List<String> ALLOWLIST = Arrays.asList("echo", "ping", "hostname", "ls");

    


    public String handleBy(String input) {
        if (input == null) {
            return "noop";
        }
        String name = input.split("\\s+")[0];
        if (!ALLOWLIST.contains(name)) {
            throw new IllegalArgumentException("command not allowed: " + name);
        }
        /*ANCHOR_1*/
        // 仅执行常量命令字面值，用户输入已被白名单过滤，不可达 sink
        String result = execConst(new String[]{"echo", "localhost-demo"});
        return result;
    }

    private String execConst(String[] cmd) {
        return "ran: " + String.join(" ", cmd);
    }

    public static void main(String[] args) {
        new ChainControllerBy().handleBy("echo hi");
    }
}
