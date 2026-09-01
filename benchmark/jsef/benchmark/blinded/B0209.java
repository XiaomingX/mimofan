
package blinded;









public class ConditionalAuthzBypass_By {

    public Object adminResource(String callerRole) {
        /*ANCHOR_1*/
        if (!"ADMIN".equals(callerRole)) {
            return "DENIED"; // 角色校验始终生效，sink 不可达
        }
        System.out.println("[abstract admin action] executed");
        return "OK";
    }
}
