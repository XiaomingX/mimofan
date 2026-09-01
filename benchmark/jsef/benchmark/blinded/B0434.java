
package blinded;








public class TenantIsolationService {

    
    public boolean belongsTo(String callerTenant, String targetTenant) {
        /*ANCHOR_1*/
        return targetTenant.startsWith(callerTenant); // 弱隔离：前缀匹配可绕过
    }
}
