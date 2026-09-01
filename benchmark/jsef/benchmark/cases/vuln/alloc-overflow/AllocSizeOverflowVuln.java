package com.jsef.benchmark.vuln;

/*
 * JSEF-Benchmark L3 — 整数溢出致分配失控（integer overflow → allocation runaway）
 *
 * 难度：L3（数值语义陷阱）。size * 4 用 int 乘法：当 size 接近 Integer.MAX_VALUE/4 时，
 * 乘积回绕溢出为负值 → new byte[负数] 抛 NegativeArraySizeException（拒绝服务）；
 * 若回绕成巨值，则直接触发 OOM。攻击者仅凭一个 size 参数即可放大分配开销
 * （CWE-190 Integer Overflow → CWE-400 DoS）。
 *
 * CWE-190 (Integer Overflow or Wraparound)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用载荷。
 *
 * 修复要点（对照 AllocSizeOverflowSafe.java）：long 运算 + 上限校验（size <= MAX/4）。
 */
public class AllocSizeOverflowVuln {

    /**
     * 按用户传入的 size 分配缓冲区。
     *
     * @param userSize 用户可控大小（意图传接近 Integer.MAX_VALUE/4 触发溢出）
     */
    public void allocate(String userSize) {
        int size = Integer.parseInt(userSize);
        // [VULN] 危险终点：int 乘法溢出后 size*4 为负/巨值，导致拒绝服务或 OOM
        // [CHECKPOINT id=JSEF-ALLOC-001 cwe=190 level=L3 source=size param sink=new byte[size*4] overflow → giant allocation/DoS expect=VULN]
        byte[] buf = new byte[size * 4];
        System.out.println("[alloc] bytes=" + buf.length);
    }

    public static void main(String[] args) {
        new AllocSizeOverflowVuln().allocate(String.valueOf(Integer.MAX_VALUE / 4 + 1));
    }
}
