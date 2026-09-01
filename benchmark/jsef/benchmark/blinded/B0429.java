
package blinded;







public class SystemResourceService {

    
    public String purgeCache() {
        // 语义等价：cacheManager.getCache("system").clear();
        /*ANCHOR_1*/
        System.out.println("[system-cache-purge] 高危管理操作被执行");
        return "purged";
    }
}
