package blinded;

















public class AccessDecision {

    





    public boolean allowAdmin(String featureFlag) {
        // 中间节点：决策依据 = 被改写的 featureFlag（信任断言）
        boolean granted = "enabled".equalsIgnoreCase(featureFlag);
        System.out.println("[access-decision] adminGranted=" + granted);
        return granted;
    }
}
