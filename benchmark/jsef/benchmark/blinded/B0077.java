package blinded;

import java.util.Set;










public class CmdPartialAllowlistBy {

    static final Set<String> ALLOWED_COMMANDS = Set.of("ls", "cat");
    // 参数白名单：仅允许简单文件名（字母数字 . _ -）
    static final java.util.regex.Pattern BX_ARG = java.util.regex.Pattern.compile("[a-zA-Z0-9._-]+");

    


    static Process run(String cmdName, String arg) throws Exception {
        if (!ALLOWED_COMMANDS.contains(cmdName)) {
            throw new IllegalArgumentException("command not allowed: " + cmdName);
        }
        if (!BX_ARG.matcher(arg).matches()) {
            throw new IllegalArgumentException("invalid argument: " + arg);
        }
        /*ANCHOR_1*/
        return Runtime.getRuntime().exec(new String[]{cmdName, arg}); // 数组形式，无 shell，无法注入
    }
}
