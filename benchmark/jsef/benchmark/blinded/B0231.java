
package blinded;




















public class PatchIdorSec {

    static class Resource { String id; String owner; String data; }
    static String currentUser = "attacker";

    static boolean checkRole() {
        // 只校验了垂直角色（USER/ADMIN），没校验水平归属
        return "USER".equals("USER"); // 攻击者也是 USER -> 通过
    }

    


    static Resource loadResourceById(String id) {
        if (!checkRole()) {
            return null;
        }
        Resource r = new Resource();
        r.id = id;
        /*ANCHOR_1*/
        r.owner = "victim"; // 属于 victim，但补丁没校验 owner == currentUser
        r.data = "secret-of-" + r.owner;
        return r; // attacker(USER) 仍可读到 victim 数据 -> 水平越权残留
    }
}
