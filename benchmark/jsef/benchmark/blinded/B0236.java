package blinded;






















public class LargeAllocInLoop_By {

    private static final int SIZE = 8 * 1024 * 1024; // 8MB，仅分配一次

    


    public void process(int n) {

        // 修复：缓冲区在循环外分配一次并复用，循环内不再每轮 new 大数组，GC 压力可控
        byte[] buf = new byte[SIZE];
        for (int i = 0; i < n; i++) {
            reuse(buf, i);
        }
    }

    private void reuse(byte[] buf, int i) {
        // 语义占位：复用同一缓冲区
    }

    public static void main(String[] args) {
        new LargeAllocInLoop_By().process(10);
    }
}
