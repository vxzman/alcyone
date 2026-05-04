/*
 * ifaddr6 - FreeBSD IPv6 address discovery
 *
 * Uses getifaddrs() to enumerate interfaces and
 * ioctl(SIOCGIFALIFETIME_IN6) to query lifetimes,
 * ioctl(SIOCGIFAFLAG_IN6) to query flags (temporary).
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/ioctl.h>
#include <net/if.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <ifaddrs.h>
#include <time.h>
#include <errno.h>
#include <limits.h>

#if defined(__FreeBSD__)
#include <netinet6/in6_var.h>
#endif

#ifndef ND6_INFINITE_LIFETIME
#define ND6_INFINITE_LIFETIME 0xffffffffU
#endif

#ifndef INET6_ADDRSTRLEN
#define INET6_ADDRSTRLEN 46
#endif

#ifndef IN6_IFF_TEMPORARY
#define IN6_IFF_TEMPORARY  0x0020
#endif

typedef struct {
    char addr[INET6_ADDRSTRLEN];
    char iface[IFNAMSIZ];
    unsigned int preferred_lft;   /* seconds, ND6_INFINITE_LIFETIME = forever */
    unsigned int valid_lft;       /* seconds */
    unsigned char is_temporary;   /* 1 = privacy/temporary address */
} ifaddr6_entry;

static void get_iface_addr_flags(int s, const char *ifname, const struct sockaddr_in6 *sin6,
                                  unsigned int *pltime, unsigned int *vltime, unsigned char *is_temporary) {
    time_t now = time(NULL);

#if defined(__FreeBSD__)
    struct in6_ifreq ifr6;
    memset(&ifr6, 0, sizeof(ifr6));
    strncpy(ifr6.ifr_name, ifname, IFNAMSIZ - 1);
    ifr6.ifr_addr = *sin6;

    /* Query lifetime */
    int ret = ioctl(s, SIOCGIFALIFETIME_IN6, &ifr6);
    if (ret == 0) {
        struct in6_addrlifetime lt = ifr6.ifr_ifru.ifru_lifetime;
        fprintf(stderr, "[ifaddr6 ioctl] SIOCGIFALIFETIME_IN6 OK: expire=%ld preferred=%ld\n",
                (long)lt.ia6t_expire, (long)lt.ia6t_preferred);
        if (lt.ia6t_preferred != (time_t)-1 && lt.ia6t_preferred > now)
            *pltime = (unsigned int)(lt.ia6t_preferred - now);
        if (lt.ia6t_expire != (time_t)-1 && lt.ia6t_expire > now)
            *vltime = (unsigned int)(lt.ia6t_expire - now);
    } else {
        fprintf(stderr, "[ifaddr6 ioctl] SIOCGIFALIFETIME_IN6 FAILED: errno=%d (%s)\n",
                errno, strerror(errno));
    }

    /* Query flags (including IN6_IFF_TEMPORARY) */
    memset(&ifr6, 0, sizeof(ifr6));
    strncpy(ifr6.ifr_name, ifname, IFNAMSIZ - 1);
    ifr6.ifr_addr = *sin6;

    ret = ioctl(s, SIOCGIFAFLAG_IN6, &ifr6);
    if (ret == 0) {
        *is_temporary = (ifr6.ifr_ifru.ifru_flags6 & IN6_IFF_TEMPORARY) ? 1 : 0;
        fprintf(stderr, "[ifaddr6 ioctl] SIOCGIFAFLAG_IN6 OK: flags6=0x%x, temp=%u\n",
                ifr6.ifr_ifru.ifru_flags6, *is_temporary);
    } else {
        fprintf(stderr, "[ifaddr6 ioctl] SIOCGIFAFLAG_IN6 FAILED: errno=%d (%s)\n",
                errno, strerror(errno));
    }
#endif
}

int ifaddr6_query(const char *ifname, ifaddr6_entry *results, int max_results, int *error_code) {
    *error_code = 0;

    /* Validate interface exists */
    if (if_nametoindex(ifname) == 0) {
        *error_code = 1;
        return -1;
    }

    /* Create socket BEFORE getifaddrs - required on FreeBSD for ioctl to work */
    int s = socket(AF_INET6, SOCK_DGRAM, 0);
    if (s == -1) {
        *error_code = 2;
        return -1;
    }

    struct ifaddrs *ifap = NULL;
    if (getifaddrs(&ifap) == -1) {
        close(s);
        *error_code = 2;
        return -1;
    }

    int count = 0;

    for (struct ifaddrs *ifa = ifap; ifa != NULL; ifa = ifa->ifa_next) {
        if (ifa->ifa_addr == NULL ||
            ifa->ifa_addr->sa_family != AF_INET6) {
            continue;
        }

        /* Match interface name */
        if (strcmp(ifa->ifa_name, ifname) != 0) {
            continue;
        }

        struct sockaddr_in6 *sin6 = (struct sockaddr_in6 *)ifa->ifa_addr;
        struct in6_addr addr = sin6->sin6_addr;

        /* Skip link-local (fe80::/10) */
        if (addr.s6_addr[0] == 0xfe && (addr.s6_addr[1] & 0xc0) == 0x80)
            continue;

        /* Skip loopback (::1) */
        if (memcmp(addr.s6_addr, "\x00\x00\x00\x00\x00\x00\x00\x00"
                                  "\x00\x00\x00\x00\x00\x00\x00\x01", 16) == 0)
            continue;

        /* Skip ULA (fc00::/7) */
        if ((addr.s6_addr[0] & 0xfe) == 0xfc)
            continue;

        /* Format address string */
        char addr_str[INET6_ADDRSTRLEN];
        if (inet_ntop(AF_INET6, &addr, addr_str, sizeof(addr_str)) == NULL)
            continue;

        /* Query lifetime and flags via ioctl */
        unsigned int pltime = ND6_INFINITE_LIFETIME;
        unsigned int vltime = ND6_INFINITE_LIFETIME;
        unsigned char is_temp = 0;

        get_iface_addr_flags(s, ifname, sin6, &pltime, &vltime, &is_temp);

        fprintf(stderr, "[ifaddr6 DEBUG] %s pltime=%u vltime=%u temp=%u\n",
                addr_str, pltime, vltime, is_temp);

        if (count < max_results) {
            strncpy(results[count].addr, addr_str, INET6_ADDRSTRLEN - 1);
            results[count].addr[INET6_ADDRSTRLEN - 1] = '\0';
            strncpy(results[count].iface, ifname, IFNAMSIZ - 1);
            results[count].iface[IFNAMSIZ - 1] = '\0';
            results[count].preferred_lft = pltime;
            results[count].valid_lft = vltime;
            results[count].is_temporary = is_temp;
            count++;
        }
    }

    close(s);
    freeifaddrs(ifap);

    return count;
}
