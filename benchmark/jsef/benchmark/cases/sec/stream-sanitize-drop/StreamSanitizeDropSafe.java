package com.jsef.benchmark.sec;

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
public class StreamSanitizeDropSafe {

    static String sanitize(String arg) {
        return arg.replaceAll("[;&|$`]", "");
    }

    /**
     * 安全入口：消毒结果 collect 回传 args 再执行。
     *
     * @param args 用户可控命令参数
     */
    public static void run(List<String> args) throws Exception {
        args = args.stream().map(StreamSanitizeDropSafe::sanitize).collect(Collectors.toList()); // 消毒生效
        String cmd = String.join(" ", args); // 使用消毒后的参数
        // [CHECKPOINT id=JSEF-STREAMSAN-001S cwe=78 level=L3 source=user command args sink=Runtime.exec(String.join after collected map sanitize) expect=SAFE]
        Runtime.getRuntime().exec(cmd); // 命令仅含消毒后参数，无法注入
    }

    public static void main(String[] args) throws Exception {
        run(Arrays.asList("ls", "-l", "; echo pwned"));
    }
}
