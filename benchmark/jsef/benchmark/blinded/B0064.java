
package blinded;




public class UserStoreBy {

    public String persistRole(String userId, String role) {
        // 语义等价：UPDATE users SET role=? WHERE id=?  (仅 ADMIN 可达)
        System.out.println("[db-update][authorized] UPDATE users SET role='" + role + "' WHERE id='" + userId + "'");
        return "role-updated";
    }
}
