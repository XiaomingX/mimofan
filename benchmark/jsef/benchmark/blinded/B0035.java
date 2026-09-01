
package blinded;







public class ActuatorExposureBy {

    


    static String byConfig() {
        /*ANCHOR_1*/
        return "management.endpoints.web.exposure.include=health,info"; // 配合 spring-security 认证
    }
}
