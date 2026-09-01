
package blinded;















public class PatchIdorBx {

    static class Resource { String id; String owner; String data; }

    
    static String currentUser = "attacker";

    


    static Resource loadResourceById(String id) {
        Resource r = new Resource();
        r.id = id;
        /*ANCHOR_1*/
        r.owner = "victim"; // 实际属于 victim，但代码未校验 owner
        r.data = "secret-of-" + r.owner;
        return r; // 攻击者拿到 victim 的数据 -> 水平越权
    }
}
