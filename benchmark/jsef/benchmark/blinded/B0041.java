package blinded;

/*
 * JSEF-Benchmark L3 — 整数溢出致分配失控修复（CWE-190）
 *
 * 修复：改用 long 运算并在分配前做上限校验（size <= MAX_ALLOC，其中
 * MAX_ALLOC = Integer.MAX_VALUE / 4），保证 size * 4 恒在 int 正数域内，
 * 负数（NegativeArraySizeException）与回绕巨值（OOM）都被拦截。
 *
 * CWE-190 (Integer Overflow or Wraparound)。
 */
public class AllocSizeOverflowBy {

    // 分配上限：确保 size * 4 不越界，杜绝溢出
    static final long MAX_ALLOC = Integer.MAX_VALUE / 4L;

    




    public void allocate(String userSize) {
        long size = Long.parseLong(userSize);
        if (size <= 0 || size > MAX_ALLOC) {
            throw new IllegalArgumentException("[alloc] size out of range: " + size);
        }

        byte[] buf = new byte[(int) (size * 4)];
        System.out.println("[alloc] bytes=" + buf.length);
    }

    public static void main(String[] args) {
        new AllocSizeOverflowBy().allocate("1024");
    }
}
