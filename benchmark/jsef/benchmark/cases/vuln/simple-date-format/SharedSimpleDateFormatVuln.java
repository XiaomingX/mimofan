package com.jsef.benchmark.vuln.sdf;

import java.text.ParseException;
import java.text.SimpleDateFormat;
import java.util.Date;

/*
 * JSEF-Benchmark L3 — 共享非线程安全 SimpleDateFormat（CWE-567）
 *
 * 难度：L3（跨线程数据流）。static 字段 fmt 被多个线程并发 parse，
 * SimpleDateFormat 内部可变 Calendar 状态被交叉污染，解析结果错乱，
 * 过期 token 的时间可能被解析成未来时间，从而被误判为未过期放行。
 *
 * CWE-567 (Unsynchronized Shared State in a Multithreaded Context)。
 * 安全底线：仅 localhost 演示语义，不写真实攻击利用脚本。
 *
 * 修复要点（对照 SharedSimpleDateFormatSafe.java）：ThreadLocal 隔离或 java.time 不可变类。
 */
public class SharedSimpleDateFormatVuln {

    // [VULN] 漏洞点：static 共享可变 SimpleDateFormat 被多线程并发使用
    private static final SimpleDateFormat fmt = new SimpleDateFormat("yyyy-MM-dd HH:mm:ss");

    /**
     * 校验 token 是否过期。
     *
     * @param tokenExpiryStr 用户可控的 token 过期时间字符串（不可信源）
     * @param now            当前时间戳
     */
    public boolean isTokenValid(String tokenExpiryStr, long now) {
        Date expiry;
        try {
            expiry = fmt.parse(tokenExpiryStr); // 并发 parse：共享 Calendar 被交叉污染
        } catch (ParseException e) {
            return false;
        }
        // [CHECKPOINT id=JSEF-SDF-001 cwe=567 level=L3 source=concurrent parse of token expiry sink=shared SimpleDateFormat corrupted date comparison expect=VULN trace=benchmark/cases/vuln/simple-date-format/SharedSimpleDateFormatVuln.java:22,benchmark/cases/vuln/simple-date-format/SharedSimpleDateFormatVuln.java:33,benchmark/cases/vuln/simple-date-format/SharedSimpleDateFormatVuln.java:38]
        return expiry.before(new Date(now)); // 过期判断：污染致过期 token 误判未过期
    }

    public static void main(String[] args) {
        // 仅 localhost 演示：并发校验同一共享 fmt
        SharedSimpleDateFormatVuln checker = new SharedSimpleDateFormatVuln();
        System.out.println(checker.isTokenValid("2026-08-25 00:00:00", System.currentTimeMillis()));
    }
}
