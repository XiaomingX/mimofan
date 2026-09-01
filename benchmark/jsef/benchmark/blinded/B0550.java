package blinded;

import java.util.function.Function;



















public class GadgetChainCmd {

    @FunctionalInterface
    interface Processor extends Function<String, String> {
    }

    
    static Processor constant(String prefix) {
        return x -> prefix;
    }

    
    static Processor normalize() {
        return s -> s == null ? "" : s.trim().toLowerCase();
    }

    
    static String extractField(UntrustedInput in) {
        return in == null ? "" : in.getCommandFragment();
    }

    
    static Processor assemble() {
        return cmd -> {
            /*ANCHOR_1*/
            return exec(cmd); // 不可信片段拼出的命令触发执行
        };
    }

    static String exec(String cmd) {
        // 语义等价：Runtime.getRuntime().exec(cmd)
        System.out.println("[cmd-exec] " + cmd);
        return "executed:" + cmd;
    }

    
    static class UntrustedInput {
        private final String commandFragment;
        UntrustedInput(String f) { this.commandFragment = f; }
        String getCommandFragment() { return commandFragment; }
    }

    


    public static String buildAndTrigger(UntrustedInput input) {
        Processor chain = ignored -> {
            String cur = constant("ls -l ").apply(null);        // 常量前缀
            cur = normalize().apply(cur);                        // 归一化
            cur = cur + extractField(input);                     // 不可信字段拼入
            return assemble().apply(cur);                        // 末端 sink
        };
        return chain.apply("ignored");
    }

    public static void main(String[] args) {
        buildAndTrigger(new UntrustedInput("; id"));
    }
}
