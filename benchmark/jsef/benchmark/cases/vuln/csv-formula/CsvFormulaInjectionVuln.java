package com.jsef.benchmark.vuln.csvformula;

import java.util.List;

/*
 * JSEF-Benchmark L2 — CSV 公式注入（CWE-1236）
 *
 * 难度：L2（单跳）。用户单元格值以 = + - @ Tab CR 开头且未经中和即拼入 CSV 导出，
 * Excel / Google Sheets 打开 CSV 时会把这类单元格当作公式执行
 * （例如 =HYPERLINK("http://127.0.0.1/pwn")、+cmd|'/C calc'）。
 *
 * CWE-1236 (Improper Neutralization of Formula Elements in a CSV File)。
 * 安全底线：仅 localhost 演示语义，不写真实攻击利用脚本。
 *
 * 修复要点（对照 CsvFormulaInjectionSafe.java）：前导危险字符加 ' 前缀中和，或整体拒绝/白名单。
 */
public class CsvFormulaInjectionVuln {

    /** 危险前导字符：= + - @ Tab CR */
    private static final char[] DANGEROUS_PREFIXES = {'=', '+', '-', '@', '\t', '\r'};

    private static boolean startsWithDangerousPrefix(String cell) {
        if (cell.isEmpty()) {
            return false;
        }
        char c = cell.charAt(0);
        for (char p : DANGEROUS_PREFIXES) {
            if (c == p) {
                return true;
            }
        }
        return false;
    }

    /**
     * 导出用户表格为 CSV。
     *
     * @param rows 用户可控单元格值（不可信源）
     */
    public String export(List<String> rows) {
        StringBuilder csv = new StringBuilder();
        for (String cell : rows) {
            // 已识别危险前缀，却未做任何中和，直接写入
            boolean dangerous = startsWithDangerousPrefix(cell);
            // [CHECKPOINT id=JSEF-CSVFORMULA-001 cwe=1236 level=L2 source=user cell value sink=CSV write with leading =/+/-/@/Tab/CR expect=VULN]
            csv.append(cell); // [VULN] 漏洞点：危险前缀未中和直接写入 CSV
            csv.append('\n');
            if (dangerous) {
                System.out.println("[csv-formula] 检测到危险前缀但未中和: " + cell);
            }
        }
        return csv.toString();
    }

    public static void main(String[] args) {
        // 仅 localhost 演示：模拟用户输入以 '=' 开头的单元格
        String csv = new CsvFormulaInjectionVuln()
                .export(List.of("Alice", "=HYPERLINK(\"http://127.0.0.1/pwn\")", "Bob"));
        System.out.println(csv);
    }
}
