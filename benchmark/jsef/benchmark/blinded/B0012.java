package blinded;










public class JpaDerivedQueryBy {

    private static final String FIELD_USERNAME = "Username";
    private static final String FIELD_ID = "Id";
    private static final String FIELD_EMAIL = "Email";


    private static String executeDerivedQuery(String methodName, Object arg) {
        return methodName + " -> " + arg;
    }

    




    public String findByBy(String field, Object value) {
        // 白名单映射：仅允许固定字段名，其余抛异常
        String mapped;
        switch (field) {
            case "username": mapped = FIELD_USERNAME; break;
            case "id":       mapped = FIELD_ID;       break;
            case "email":    mapped = FIELD_EMAIL;    break;
            default:
                throw new IllegalArgumentException("unsupported field: " + field);
        }
        String queryMethod = "findBy" + mapped; // 拼接的是白名单常量，污点已被阻断
        /*ANCHOR_1*/
        return executeDerivedQuery(queryMethod, value); // 方法名来自常量映射，不可达注入
    }

    public static void main(String[] args) {
        System.out.println(new JpaDerivedQueryBy().findByBy("id", "1")); // localhost 演示
    }
}
