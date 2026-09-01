package com.jsef.benchmark.sec.sdf;

import java.text.ParseException;
import java.text.SimpleDateFormat;
import java.util.Date;

/*
 * JSEF-Benchmark L3 — 共享非线程安全 SimpleDateFormat 修复（CWE-567）
 *
 * 修复：ThreadLocal 为每个线程隔离独立的 SimpleDateFormat 实例，
 * 消除跨线程 Calendar 状态污染；亦可改用 java.time 不可变类。
 *
 * CWE-567 (Unsynchronized Shared State in a Multithreaded Context)。
 */
public class SharedSimpleDateFormatSafe {

    // ThreadLocal：每线程独立实例，无共享可变状态
    private static final ThreadLocal<SimpleDateFormat> FMT = ThreadLocal.withInitial(
            () -> new SimpleDateFormat("yyyy-MM-dd HH:mm:ss"));

    /**
     * 校验 token 是否过期（线程安全）。
     *
     * @param tokenExpiryStr 用户可控的 token 过期时间字符串（不可信源）
     * @param now            当前时间戳
     */
    public boolean isTokenValid(String tokenExpiryStr, long now) {
        Date expiry;
        try {
            expiry = FMT.get().parse(tokenExpiryStr); // 每线程独立实例，无污染
        } catch (ParseException e) {
            return false;
        }
        // [CHECKPOINT id=JSEF-SDF-001S cwe=567 level=L3 source=concurrent parse of token expiry sink=ThreadLocal SimpleDateFormat thread-safe parse expect=SAFE]
        return expiry.before(new Date(now));
    }
}
