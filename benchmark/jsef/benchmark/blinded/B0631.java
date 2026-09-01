package blinded;






















public class LargeAllocInLoop {

    private static final int SIZE = 8 * 1024 * 1024; // 8MB 每轮

    


    public void process(int n) {
        for (int i = 0; i < n; i++) {

            // 缺陷：循环体内每轮分配大数组，短命大对象迫使 GC 频繁回收，吞吐骤降甚至 OOM
            byte[] buf = new byte[SIZE];
            consume(buf, i);
        }
    }

    private void consume(byte[] buf, int i) {
        // 语义占位：使用缓冲区
    }

    public static void main(String[] args) {
        new LargeAllocInLoop().process(10);
    }
}
