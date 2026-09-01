package com.jsef.benchmark.sec.csvformula;

import java.util.List;

/*
 * JSEF-Benchmark L2 — CSV 公式注入修复（CWE-1236）
 *
 * 修复：危险前导字符（= + - @ Tab CR）前加单引号 ' 前缀中和，
 * 使 Excel 将单元格按文本而非公式解释；亦可整体拒绝。
 *
 * CWE-1236 (Improper Neutralization of Formula Elements in a CSV File)。
 */
public class CsvFormulaInjectionSafe {

    private static final char[] DANGEROUS_PREFIXES = {'=', '+', '-', '@', '\t', '\r'};

    /** 危险前缀前加单引号中和，安全单元格原样返回 */
    private static String sanitizeCell(String cell) {
        if (cell.isEmpty()) {
            return cell;
        }
        char c = cell.charAt(0);
        for (char p : DANGEROUS_PREFIXES) {
            if (c == p) {
                return "'" + cell; // 前导 ' 前缀：单元格按文本处理
            }
        }
        return cell;
    }

    /**
     * 导出用户表格为 CSV（已中和）。
     *
     * @param rows 用户可控单元格值（不可信源）
     */
    public String export(List<String> rows) {
        StringBuilder csv = new StringBuilder();
        for (String cell : rows) {
            // [CHECKPOINT id=JSEF-CSVFORMULA-001S cwe=1236 level=L2 source=user cell value sink=sanitize before CSV write expect=SAFE]
            csv.append(sanitizeCell(cell)); // 中和后再写入，无公式执行风险
            csv.append('\n');
        }
        return csv.toString();
    }

    public static void main(String[] args) {
        // 仅 localhost 演示：危险前缀被中和为文本
        String csv = new CsvFormulaInjectionSafe()
                .export(List.of("Alice", "=HYPERLINK(\"http://127.0.0.1/pwn\")", "Bob"));
        System.out.println(csv);
    }
}
