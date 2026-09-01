package blinded;

/*
 * JSEF-Benchmark L2 — 浮点金额比较精度丢失（double ==）
 *
 * 难度：L2（数值语义陷阱）。用 double 表示金额并以 `==` 比较，浮点二进制表示
 * 误差（如 0.1 + 0.2 != 0.3）会导致相等判断误判，使本应失败的金额校验通过或
 * 本应相等的金额被判为不等。
 *
 * CWE-682 (Incorrect Calculation)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用载荷。
 *
 * 修复要点（对照 FloatMoneyBy.java）：使用 BigDecimal 并以 compareTo == 0 比较。
 */
public class FloatMoney {

    





    public void verify(double balance, double expectedAmount) {
        // 危险终点：double 精度误差使 == 比较不可靠（演示 0.1+0.2 != 0.3）
        if (balance == expectedAmount) {
            /*ANCHOR_1*/
            grant(balance);
        }
    }

    // 抽象 sink：语义等价 放行/扣费
    static void grant(double amount) {
        System.out.println("[grant] amount=" + amount);
    }

    public static void main(String[] args) {
        new FloatMoney().verify(0.1 + 0.2, 0.3);
    }
}
