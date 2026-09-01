package blinded;

import java.util.Arrays;
import java.util.List;
import java.util.stream.Collectors;

/*
 * JSEF-Benchmark L3 — Stream 消毒结果生效（CWE-78 修复）
 *
 * 修复：args = args.stream().map(sanitize).collect(Collectors.toList())
 * 把消毒结果写回 args，随后 join/exec 使用消毒后的参数，注入被消除。
 *
 * CWE-78 (OS Command Injection)。安全底线：仅 localhost 演示语义。
 */
public class StreamSanitizeDropBy {

    static String sanitize(String arg) {
        return arg.replaceAll("[;&|$`]", "");
    }

    




    public static void run(List<String> args) throws Exception {
        args = args.stream().map(StreamSanitizeDropBy::sanitize).collect(Collectors.toList()); // 消毒生效
        String cmd = String.join(" ", args); // 使用消毒后的参数
        /*ANCHOR_1*/
        Runtime.getRuntime().exec(cmd); // 命令仅含消毒后参数，无法注入
    }

    public static void main(String[] args) throws Exception {
        run(Arrays.asList("ls", "-l", "; echo pwned"));
    }
}
