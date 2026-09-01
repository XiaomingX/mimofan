package blinded;

import java.io.IOException;
import java.util.Arrays;
import java.util.List;









public class TaintCrossMethodBy {

    private static final List<String> ALLOWLIST = Arrays.asList("echo", "ping", "hostname");

    private String methodA(String input) {
        String name = input.split("\\s+")[0];
        return ALLOWLIST.contains(name) ? "echo localhost-demo" : null;
    }

    private void methodB(String cmd) throws IOException {
        if (cmd == null) {
            throw new IllegalArgumentException("command not allowed");
        }
        /*ANCHOR_1*/
        Process p = Runtime.getRuntime().exec(new String[]{"echo", "localhost-demo"});
    }

    public void runCommand(String userInput) throws IOException {
        String shaped = methodA(userInput);
        methodB(shaped);
    }

    public static void main(String[] args) throws IOException {
        new TaintCrossMethodBy().runCommand("echo hi");
    }
}
