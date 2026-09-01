// [VULN]
/*
 * JSEF Benchmark 样本 — Thymeleaf 片段视图名 SSTI（CWE-1336，L3）
 *
 * 视图名由固定前缀 + 用户可控片段名拼接而成，如 "page :: " + section。
 * Thymeleaf 的 ViewResolver 在解析视图名时，遇到 "::" 片段选择器，
 * 会把片段表达式部分当作 Spring EL / OGNL 求值（片段选择器 ~{...} 与
 * ${...} 均会被计算），从而形成服务端模板注入（SSTI）。
 *
 * 关键语义：视图名解析时机 = 表达式求值时机。
 * 安全底线：仅 localhost 演示语义，不提供真实利用 payload。
 * 修复要点（对照 ThymeleafFragmentSafe.java）：片段名白名单固定集，禁止 :: 前缀拼接。
 */
package com.jsef.benchmark.vuln;

import org.thymeleaf.TemplateEngine;
import org.thymeleaf.context.Context;

import java.io.StringWriter;
import java.io.Writer;

public class ThymeleafFragmentVuln {

    private final TemplateEngine templateEngine = new TemplateEngine();

    /**
     * 片段解析（等价 Spring Thymeleaf ViewResolver 内部对 viewName 的片段解析）：
     * viewName 含 "::" 时，片段表达式部分进入表达式引擎。
     */
    static String resolveFragment(String viewName) {
        // 视图名解析时机 = 片段解析：:: 之后的内容作为表达式保留
        return viewName;
    }

    /**
     * 危险入口：section 用户可控，拼入视图名触发表达式求值。
     *
     * @param section HTTP 请求参数，如 ?section=footer（可被替换为表达式载荷）
     */
    public String render(String section) {
        // [VULN] 用户可控 section 拼入视图名 → 片段表达式 SSTI
        // node1: 拼接视图名 "page :: " + section
        String viewName = "page :: " + section;
        // node2: 片段解析（含 :: 片段选择器 → 表达式进入求值路径）
        String fragment = resolveFragment(viewName);
        Writer writer = new StringWriter();
        // node3/sink: 表达式求值（process 渲染 fragment 时按 Spring EL 计算）
        // [CHECKPOINT id=JSEF-THYFRAG-001 cwe=1336 level=L3 source=user section fragment sink=TemplateEngine.process("page :: "+section) expression eval expect=VULN trace=benchmark/cases/vuln/thymeleaf-fragment/ThymeleafFragmentVuln.java:43,benchmark/cases/vuln/thymeleaf-fragment/ThymeleafFragmentVuln.java:45,benchmark/cases/vuln/thymeleaf-fragment/ThymeleafFragmentVuln.java:49]
        templateEngine.process(fragment, new Context(), writer);
        return writer.toString();
    }
}
