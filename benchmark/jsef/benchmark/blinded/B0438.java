package blinded;














public class ErrorBasedInfoLeak {

    


    static String handle(Exception e) {
        /*ANCHOR_1*/
        return "error: " + e.getMessage(); // 泄露内部实现细节（SQL/路径/表名）
    }
}
