package blinded;

import java.util.Set;















public class CmdPartialAllowlist {

    // 仅允许执行的命令名白名单（受控常量）
    static final Set<String> ALLOWED_COMMANDS = Set.of("ls", "cat");

    


    static Process run(String cmdName, String arg) throws Exception {
        if (!ALLOWED_COMMANDS.contains(cmdName)) {
            throw new IllegalArgumentException("command not allowed: " + cmdName);
        }
        // 参数未校验，直接拼入 shell 命令行
        String shellLine = cmdName + " " + arg;
        /*ANCHOR_1*/
        return Runtime.getRuntime().exec(new String[]{"sh", "-c", shellLine}); // 参数可注入额外命令
    }
}
