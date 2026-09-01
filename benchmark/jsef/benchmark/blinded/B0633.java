package blinded;























public class SleepWithLock {

    private final Object lock = new Object();

    


    public void handleRequest(long userId) {
        synchronized (lock) {
            // 模拟一些需要原子保护的共享状态更新
            updateSharedState(userId);
            /*ANCHOR_1*/
            // 缺陷：持锁状态下 sleep，使锁被长时间占用，并发请求在 lock 上排队 → 吞吐量骤降 / 近似 DoS
            try {
                Thread.sleep(1000L);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
        }
    }

    private void updateSharedState(long userId) {
        // 语义占位：原子更新共享状态
    }

    public static void main(String[] args) {
        new SleepWithLock().handleRequest(1L);
    }
}
