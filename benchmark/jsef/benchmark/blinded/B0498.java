package blinded;



















public class GmExpectClassBlocked {

    



    // 语义等价：javax.sql.RowSet 实现类（如 JdbcRowSetImpl），fastjson 1.2.68+ 下经 expectClass 入口会被封堵。
    public static class RowSetGadgetStub {
        // 占位：模拟 RowSet 实现类的危险字段（dataSourceName / autoCommit）
    }

    


    public static Object resolveViaExpectClass(String typeName) {
        Class<?> clazz = loadClass(typeName);   // 加载目标类

        /*ANCHOR_1*/
        if (isBlockedInterface(clazz)) {   // 接口判定行：ClassLoader/DataSource/RowSet -> 封堵
            throw new UnsupportedOperationException(
                    "expectClass block: " + typeName + " implements blocked interface (1.2.68+)");
        }
        return instantiate(clazz);   // 抛错行：blocked 类不会到达此处
    }

    
    private static Class<?> loadClass(String typeName) {
        System.out.println("[demo-only] loadClass: " + typeName);
        return RowSetGadgetStub.class;  // 占位：假定加载到 RowSet 实现类
    }

    
    private static boolean isBlockedInterface(Class<?> clazz) {
        // 占位判定：实际 fastjson 检查 ClassLoader / DataSource / RowSet
        return true;  // 演示：RowSet 实现类被封堵
    }

    
    private static Object instantiate(Class<?> clazz) {
        System.out.println("[demo-only] instantiate blocked type (unreachable): " + clazz);
        return null;
    }
}
