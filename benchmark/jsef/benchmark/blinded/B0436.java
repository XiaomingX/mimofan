
package blinded;







public class UserStore {

    
    public String persistRole(String userId, String role) {
        // 语义等价：jdbcTemplate.update("UPDATE users SET role=? WHERE id=?", role, userId)
        /*ANCHOR_1*/
        System.out.println("[db-update] UPDATE users SET role='" + role + "' WHERE id='" + userId + "'");
        return "role-updated";
    }
}
