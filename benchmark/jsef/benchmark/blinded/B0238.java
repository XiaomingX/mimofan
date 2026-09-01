package blinded;






















public class SleepWithLock_By {

    private final Object lock = new Object();

    


    public void handleRequest(long userId) {
        synchronized (lock) {
            updateSharedState(userId);
        }
        /*ANCHOR_1*/
        // 修复：sleep 在锁外执行，临界区仅做原子更新，锁持有时间极短，吞吐不受阻塞影响
        try {
            Thread.sleep(1000L);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }

    private void updateSharedState(long userId) {
        // 语义占位：原子更新共享状态
    }

    public static void main(String[] args) {
        new SleepWithLock_By().handleRequest(1L);
    }
}
